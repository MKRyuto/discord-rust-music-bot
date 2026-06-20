use std::{
    collections::{HashMap, VecDeque},
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

#[derive(Clone, Debug)]
pub struct HistoryTrack {
    pub title: String,
    pub url: String,
    pub duration_secs: Option<u64>,
    pub thumbnail: Option<String>,
    pub play_count: usize,
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
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        let conn = self.connect()?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS guild_settings (
                guild_id TEXT PRIMARY KEY,
                volume_percent INTEGER NOT NULL DEFAULT 100,
                autoplay_enabled INTEGER NOT NULL DEFAULT 0,
                normalize_enabled INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS dj_roles (
                guild_id TEXT NOT NULL,
                role_id TEXT NOT NULL,
                PRIMARY KEY (guild_id, role_id)
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

            CREATE TABLE IF NOT EXISTS track_history (
                guild_id TEXT NOT NULL,
                url TEXT NOT NULL,
                title TEXT NOT NULL,
                duration_secs INTEGER,
                thumbnail TEXT,
                play_count INTEGER NOT NULL DEFAULT 1,
                last_played_at INTEGER NOT NULL,
                PRIMARY KEY (guild_id, url)
            );

            CREATE TABLE IF NOT EXISTS queue_tracks (
                guild_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                title TEXT NOT NULL,
                url TEXT NOT NULL,
                duration_secs INTEGER,
                requested_by TEXT NOT NULL,
                thumbnail TEXT,
                is_now_playing INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (guild_id, position)
            );
            ",
        )?;

        if let Err(err) = conn.execute(
            "ALTER TABLE guild_settings ADD COLUMN autoplay_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        ) {
            if !err.to_string().contains("duplicate column name") {
                return Err(err.into());
            }
        }

        if let Err(err) = conn.execute(
            "ALTER TABLE guild_settings ADD COLUMN normalize_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        ) {
            if !err.to_string().contains("duplicate column name") {
                return Err(err.into());
            }
        }

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

    pub fn autoplay_enabled(&self, guild_id: serenity::GuildId) -> Result<bool, Error> {
        let conn = self.connect()?;
        let enabled = conn
            .query_row(
                "SELECT autoplay_enabled FROM guild_settings WHERE guild_id = ?1",
                params![guild_id.get().to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            != 0;

        Ok(enabled)
    }

    pub fn set_autoplay_enabled(
        &self,
        guild_id: serenity::GuildId,
        enabled: bool,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT INTO guild_settings (guild_id, autoplay_enabled)
            VALUES (?1, ?2)
            ON CONFLICT(guild_id) DO UPDATE SET autoplay_enabled = excluded.autoplay_enabled
            ",
            params![guild_id.get().to_string(), if enabled { 1 } else { 0 }],
        )?;

        Ok(())
    }

    pub fn normalize_enabled(&self, guild_id: serenity::GuildId) -> Result<bool, Error> {
        let conn = self.connect()?;
        let enabled = conn
            .query_row(
                "SELECT normalize_enabled FROM guild_settings WHERE guild_id = ?1",
                params![guild_id.get().to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            != 0;

        Ok(enabled)
    }

    pub fn set_normalize_enabled(
        &self,
        guild_id: serenity::GuildId,
        enabled: bool,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT INTO guild_settings (guild_id, normalize_enabled)
            VALUES (?1, ?2)
            ON CONFLICT(guild_id) DO UPDATE SET normalize_enabled = excluded.normalize_enabled
            ",
            params![guild_id.get().to_string(), if enabled { 1 } else { 0 }],
        )?;

        Ok(())
    }

    pub fn dj_roles(&self, guild_id: serenity::GuildId) -> Result<Vec<serenity::RoleId>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT role_id
            FROM dj_roles
            WHERE guild_id = ?1
            ORDER BY role_id ASC
            ",
        )?;

        let rows = stmt.query_map(params![guild_id.get().to_string()], |row| {
            let role_id_raw: String = row.get(0)?;
            Ok(role_id_raw.parse::<u64>().ok().map(serenity::RoleId::new))
        })?;

        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn add_dj_role(
        &self,
        guild_id: serenity::GuildId,
        role_id: serenity::RoleId,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT OR IGNORE INTO dj_roles (guild_id, role_id)
            VALUES (?1, ?2)
            ",
            params![guild_id.get().to_string(), role_id.get().to_string()],
        )?;

        Ok(())
    }

    pub fn remove_dj_role(
        &self,
        guild_id: serenity::GuildId,
        role_id: serenity::RoleId,
    ) -> Result<bool, Error> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM dj_roles WHERE guild_id = ?1 AND role_id = ?2",
            params![guild_id.get().to_string(), role_id.get().to_string()],
        )?;

        Ok(changed > 0)
    }

    pub fn record_history(&self, guild_id: serenity::GuildId, track: &Track) -> Result<(), Error> {
        let conn = self.connect()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        conn.execute(
            "
            INSERT INTO track_history (
                guild_id,
                url,
                title,
                duration_secs,
                thumbnail,
                play_count,
                last_played_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
            ON CONFLICT(guild_id, url) DO UPDATE SET
                title = excluded.title,
                duration_secs = excluded.duration_secs,
                thumbnail = excluded.thumbnail,
                play_count = track_history.play_count + 1,
                last_played_at = excluded.last_played_at
            ",
            params![
                guild_id.get().to_string(),
                track.url,
                track.title,
                track.duration_secs.map(|duration| duration as i64),
                track.thumbnail,
                now,
            ],
        )?;

        Ok(())
    }

    pub fn search_history(
        &self,
        guild_id: serenity::GuildId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryTrack>, Error> {
        let conn = self.connect()?;
        let pattern = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "
            SELECT title, url, duration_secs, thumbnail, play_count
            FROM track_history
            WHERE guild_id = ?1
                AND (?2 = '%%' OR title LIKE ?2 OR url LIKE ?2)
            ORDER BY play_count DESC, last_played_at DESC
            LIMIT ?3
            ",
        )?;

        let rows = stmt.query_map(
            params![guild_id.get().to_string(), pattern, limit as i64],
            |row| {
                let duration_secs = row
                    .get::<_, Option<i64>>(2)?
                    .and_then(|duration| u64::try_from(duration).ok());

                Ok(HistoryTrack {
                    title: row.get(0)?,
                    url: row.get(1)?,
                    duration_secs,
                    thumbnail: row.get(3)?,
                    play_count: row.get::<_, i64>(4)?.max(0) as usize,
                })
            },
        )?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn top_history(
        &self,
        guild_id: serenity::GuildId,
        limit: usize,
    ) -> Result<Vec<HistoryTrack>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT title, url, duration_secs, thumbnail, play_count
            FROM track_history
            WHERE guild_id = ?1
            ORDER BY play_count DESC, last_played_at DESC
            LIMIT ?2
            ",
        )?;

        let rows = stmt.query_map(params![guild_id.get().to_string(), limit as i64], |row| {
            let duration_secs = row
                .get::<_, Option<i64>>(2)?
                .and_then(|duration| u64::try_from(duration).ok());

            Ok(HistoryTrack {
                title: row.get(0)?,
                url: row.get(1)?,
                duration_secs,
                thumbnail: row.get(3)?,
                play_count: row.get::<_, i64>(4)?.max(0) as usize,
            })
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn random_history_track(
        &self,
        guild_id: serenity::GuildId,
        requested_by: serenity::UserId,
        exclude_url: Option<&str>,
    ) -> Result<Option<Track>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT title, url, duration_secs, thumbnail
            FROM track_history
            WHERE guild_id = ?1
                AND (?2 IS NULL OR url != ?2)
            ORDER BY RANDOM()
            LIMIT 1
            ",
        )?;

        let track = stmt
            .query_row(params![guild_id.get().to_string(), exclude_url], |row| {
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
            })
            .optional()?;

        Ok(track)
    }

    pub fn save_queue(
        &self,
        guild_id: serenity::GuildId,
        now_playing: &Option<Track>,
        queue: &VecDeque<Track>,
    ) -> Result<(), Error> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let guild_id_raw = guild_id.get().to_string();

        tx.execute(
            "DELETE FROM queue_tracks WHERE guild_id = ?1",
            params![guild_id_raw],
        )?;

        for (position, track) in now_playing.iter().chain(queue.iter()).enumerate() {
            tx.execute(
                "
                INSERT INTO queue_tracks (
                    guild_id,
                    position,
                    title,
                    url,
                    duration_secs,
                    requested_by,
                    thumbnail,
                    is_now_playing
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ",
                params![
                    guild_id_raw,
                    position as i64,
                    track.title,
                    track.url,
                    track.duration_secs.map(|duration| duration as i64),
                    track.requested_by.get().to_string(),
                    track.thumbnail,
                    if position == 0 && now_playing.is_some() {
                        1
                    } else {
                        0
                    },
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn load_all_queues(
        &self,
    ) -> Result<HashMap<serenity::GuildId, (Option<Track>, VecDeque<Track>)>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT guild_id, title, url, duration_secs, requested_by, thumbnail, is_now_playing
            FROM queue_tracks
            ORDER BY guild_id ASC, position ASC
            ",
        )?;

        let rows = stmt.query_map([], |row| {
            let guild_id_raw: String = row.get(0)?;
            let duration_secs = row
                .get::<_, Option<i64>>(3)?
                .and_then(|duration| u64::try_from(duration).ok());
            let requested_by_raw: String = row.get(4)?;

            Ok((
                guild_id_raw,
                Track {
                    title: row.get(1)?,
                    url: row.get(2)?,
                    duration_secs,
                    requested_by: serenity::UserId::new(
                        requested_by_raw.parse::<u64>().unwrap_or_default(),
                    ),
                    thumbnail: row.get(5)?,
                },
                row.get::<_, i64>(6)? != 0,
            ))
        })?;

        let mut queues: HashMap<serenity::GuildId, (Option<Track>, VecDeque<Track>)> =
            HashMap::new();

        for row in rows {
            let (guild_id_raw, track, is_now_playing) = row?;
            let Ok(guild_id) = guild_id_raw.parse::<u64>() else {
                continue;
            };

            let entry = queues
                .entry(serenity::GuildId::new(guild_id))
                .or_insert_with(|| (None, VecDeque::new()));

            if is_now_playing && entry.0.is_none() {
                entry.0 = Some(track);
            } else {
                entry.1.push_back(track);
            }
        }

        Ok(queues)
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
