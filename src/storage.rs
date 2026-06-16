use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use poise::serenity_prelude as serenity;
use rusqlite::{params, Connection, OptionalExtension};

use crate::{music::track::Track, Error};

#[derive(Clone, Debug)]
pub struct Database {
    path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PlaylistSummary {
    pub name: String,
    pub track_count: usize,
}

impl Database {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let db = Self { path: path.into() };
        db.init()?;
        Ok(db)
    }

    fn connect(&self) -> Result<Connection, Error> {
        Ok(Connection::open(&self.path)?)
    }

    fn init(&self) -> Result<(), Error> {
        if let Some(parent) = self.path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }

        let conn = self.connect()?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS guild_settings (
                guild_id TEXT PRIMARY KEY,
                volume_percent INTEGER NOT NULL DEFAULT 100
            );

            CREATE TABLE IF NOT EXISTS playlists (
                guild_id TEXT NOT NULL,
                name TEXT NOT NULL,
                created_by TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (guild_id, name)
            );

            CREATE TABLE IF NOT EXISTS playlist_tracks (
                guild_id TEXT NOT NULL,
                playlist_name TEXT NOT NULL,
                position INTEGER NOT NULL,
                title TEXT NOT NULL,
                url TEXT NOT NULL,
                duration_secs INTEGER,
                requested_by TEXT NOT NULL,
                thumbnail TEXT,
                PRIMARY KEY (guild_id, playlist_name, position),
                FOREIGN KEY (guild_id, playlist_name)
                    REFERENCES playlists(guild_id, name)
                    ON DELETE CASCADE
            );
            ",
        )?;

        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn guild_volume(&self, guild_id: serenity::GuildId) -> Result<u8, Error> {
        let conn = self.connect()?;
        let volume = conn
            .query_row(
                "SELECT volume_percent FROM guild_settings WHERE guild_id = ?1",
                params![guild_id.get().to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(100)
            .clamp(0, 200) as u8;

        Ok(volume)
    }

    pub fn set_guild_volume(
        &self,
        guild_id: serenity::GuildId,
        volume_percent: u8,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT INTO guild_settings (guild_id, volume_percent)
            VALUES (?1, ?2)
            ON CONFLICT(guild_id) DO UPDATE SET volume_percent = excluded.volume_percent
            ",
            params![guild_id.get().to_string(), volume_percent as i64],
        )?;

        Ok(())
    }

    pub fn save_playlist(
        &self,
        guild_id: serenity::GuildId,
        name: &str,
        created_by: serenity::UserId,
        tracks: &[Track],
    ) -> Result<(), Error> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let guild_id = guild_id.get().to_string();
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        tx.execute(
            "
            INSERT INTO playlists (guild_id, name, created_by, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(guild_id, name) DO UPDATE SET
                created_by = excluded.created_by,
                created_at = excluded.created_at
            ",
            params![guild_id, name, created_by.get().to_string(), now],
        )?;

        tx.execute(
            "DELETE FROM playlist_tracks WHERE guild_id = ?1 AND playlist_name = ?2",
            params![guild_id, name],
        )?;

        for (position, track) in tracks.iter().enumerate() {
            tx.execute(
                "
                INSERT INTO playlist_tracks (
                    guild_id,
                    playlist_name,
                    position,
                    title,
                    url,
                    duration_secs,
                    requested_by,
                    thumbnail
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ",
                params![
                    guild_id,
                    name,
                    position as i64,
                    track.title,
                    track.url,
                    track.duration_secs.map(|duration| duration as i64),
                    track.requested_by.get().to_string(),
                    track.thumbnail,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn load_playlist(
        &self,
        guild_id: serenity::GuildId,
        name: &str,
        requested_by: serenity::UserId,
    ) -> Result<Vec<Track>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT title, url, duration_secs, thumbnail
            FROM playlist_tracks
            WHERE guild_id = ?1 AND playlist_name = ?2
            ORDER BY position ASC
            ",
        )?;

        let rows = stmt.query_map(params![guild_id.get().to_string(), name], |row| {
            let duration_secs = row
                .get::<_, Option<i64>>(2)?
                .and_then(|duration| u64::try_from(duration).ok());

            Ok(Track {
                title: row.get(0)?,
                url: row.get(1)?,
                duration_secs,
                requested_by,
                thumbnail: row.get(3)?,
            })
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn list_playlists(
        &self,
        guild_id: serenity::GuildId,
    ) -> Result<Vec<PlaylistSummary>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT playlists.name, COUNT(playlist_tracks.position) AS track_count
            FROM playlists
            LEFT JOIN playlist_tracks
                ON playlist_tracks.guild_id = playlists.guild_id
                AND playlist_tracks.playlist_name = playlists.name
            WHERE playlists.guild_id = ?1
            GROUP BY playlists.name
            ORDER BY playlists.name ASC
            ",
        )?;

        let rows = stmt.query_map(params![guild_id.get().to_string()], |row| {
            Ok(PlaylistSummary {
                name: row.get(0)?,
                track_count: row.get::<_, i64>(1)?.max(0) as usize,
            })
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn delete_playlist(&self, guild_id: serenity::GuildId, name: &str) -> Result<bool, Error> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM playlists WHERE guild_id = ?1 AND name = ?2",
            params![guild_id.get().to_string(), name],
        )?;

        Ok(changed > 0)
    }
}
