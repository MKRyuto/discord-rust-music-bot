use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
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

pub type RestoredQueues = HashMap<serenity::GuildId, (Option<Track>, VecDeque<Track>)>;

#[derive(Clone, Debug)]
pub struct HistoryTrack {
    pub title: String,
    pub url: String,
    pub duration_secs: Option<u64>,
    pub thumbnail: Option<String>,
    pub play_count: usize,
}

#[derive(Clone, Debug)]
pub struct UserStats {
    pub user_id: serenity::UserId,
    pub tracks_played: usize,
}

#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub actor_name: String,
    pub action: String,
    pub detail: String,
    pub created_at: u64,
}

#[derive(Clone, Debug)]
pub struct FeedbackEntry {
    pub id: i64,
    pub user_id: u64,
    pub user_name: String,
    pub category: String,
    pub subject: String,
    pub message: String,
    pub status: String,
    pub created_at: u64,
}

#[derive(Clone, Debug)]
pub struct ServerStats {
    pub unique_tracks: usize,
    pub total_plays: usize,
    pub playlists: usize,
}

impl Database {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let db = Self { path: path.into() };
        db.init()?;
        Ok(db)
    }

    fn connect(&self) -> Result<Connection, Error> {
        let conn = Connection::open(&self.path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
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
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS web_sessions (
                session_id TEXT PRIMARY KEY,
                payload BLOB NOT NULL,
                expires_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_web_sessions_expires_at
                ON web_sessions(expires_at);

            CREATE TABLE IF NOT EXISTS guild_settings (
                guild_id TEXT PRIMARY KEY,
                volume_percent INTEGER NOT NULL DEFAULT 100,
                autoplay_enabled INTEGER NOT NULL DEFAULT 0,
                normalize_enabled INTEGER NOT NULL DEFAULT 1,
                play_cooldown_secs INTEGER NOT NULL DEFAULT 10,
                max_queue_per_user INTEGER NOT NULL DEFAULT 10,
                vote_skip_percent INTEGER NOT NULL DEFAULT 50,
                normalize_cap_percent INTEGER NOT NULL DEFAULT 85,
                idle_timeout_secs INTEGER NOT NULL DEFAULT 60
            );

            CREATE TABLE IF NOT EXISTS dj_roles (
                guild_id TEXT NOT NULL,
                role_id TEXT NOT NULL,
                PRIMARY KEY (guild_id, role_id)
            );

            CREATE TABLE IF NOT EXISTS allowed_channels (
                guild_id TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                PRIMARY KEY (guild_id, channel_id)
            );

            CREATE TABLE IF NOT EXISTS blocked_terms (
                guild_id TEXT NOT NULL,
                term TEXT NOT NULL,
                PRIMARY KEY (guild_id, term)
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

            CREATE TABLE IF NOT EXISTS user_track_stats (
                guild_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                tracks_played INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (guild_id, user_id)
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

            CREATE TABLE IF NOT EXISTS web_audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                guild_id TEXT NOT NULL,
                actor_id TEXT NOT NULL,
                actor_name TEXT NOT NULL,
                action TEXT NOT NULL,
                detail TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_web_audit_guild_created
                ON web_audit_log(guild_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS web_feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                user_name TEXT NOT NULL,
                category TEXT NOT NULL,
                subject TEXT NOT NULL,
                message TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'open',
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_web_feedback_created
                ON web_feedback(created_at DESC);
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
            "ALTER TABLE guild_settings ADD COLUMN normalize_enabled INTEGER NOT NULL DEFAULT 1",
            [],
        ) {
            if !err.to_string().contains("duplicate column name") {
                return Err(err.into());
            }
        }

        for statement in [
            "ALTER TABLE guild_settings ADD COLUMN play_cooldown_secs INTEGER NOT NULL DEFAULT 10",
            "ALTER TABLE guild_settings ADD COLUMN max_queue_per_user INTEGER NOT NULL DEFAULT 10",
            "ALTER TABLE guild_settings ADD COLUMN vote_skip_percent INTEGER NOT NULL DEFAULT 50",
            "ALTER TABLE guild_settings ADD COLUMN normalize_cap_percent INTEGER NOT NULL DEFAULT 85",
            "ALTER TABLE guild_settings ADD COLUMN idle_timeout_secs INTEGER NOT NULL DEFAULT 60",
        ] {
            if let Err(err) = conn.execute(statement, []) {
                if !err.to_string().contains("duplicate column name") {
                    return Err(err.into());
                }
            }
        }

        if let Err(err) = conn.execute(
            "ALTER TABLE web_feedback ADD COLUMN status TEXT NOT NULL DEFAULT 'open'",
            [],
        ) {
            if !err.to_string().contains("duplicate column name") {
                return Err(err.into());
            }
        }

        let applied_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
            params![applied_at],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (2, ?1)",
            params![applied_at],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (3, ?1)",
            params![applied_at],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (4, ?1)",
            params![applied_at],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (5, ?1)",
            params![applied_at],
        )?;

        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create_backup(&self, backup_dir: &Path, retention: usize) -> Result<PathBuf, Error> {
        std::fs::create_dir_all(backup_dir)?;
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let stem = self
            .path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("music-bot");
        let backup_path = backup_dir.join(format!("{stem}-{timestamp}.db"));
        let source = self.connect()?;
        let mut destination = Connection::open(&backup_path)?;
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        backup.run_to_completion(64, Duration::from_millis(50), None)?;
        drop(backup);
        destination.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.prune_backups(backup_dir, stem, retention.max(1))?;
        Ok(backup_path)
    }

    fn prune_backups(&self, backup_dir: &Path, stem: &str, retention: usize) -> Result<(), Error> {
        let prefix = format!("{stem}-");
        let mut backups = std::fs::read_dir(backup_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?;
                if path.is_file() && name.starts_with(&prefix) && name.ends_with(".db") {
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    Some((modified, path))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        backups.sort_by_key(|(modified, _)| *modified);
        let remove_count = backups.len().saturating_sub(retention);
        for (_, path) in backups.into_iter().take(remove_count) {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn save_web_session(
        &self,
        session_id: &str,
        payload: &[u8],
        expires_at: u64,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT INTO web_sessions (session_id, payload, expires_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(session_id) DO UPDATE SET
                payload = excluded.payload,
                expires_at = excluded.expires_at
            ",
            params![session_id, payload, expires_at as i64],
        )?;
        Ok(())
    }

    pub fn load_web_sessions(&self, now: u64) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM web_sessions WHERE expires_at <= ?1",
            params![now as i64],
        )?;
        let mut stmt =
            conn.prepare("SELECT session_id, payload FROM web_sessions WHERE expires_at > ?1")?;
        let rows = stmt.query_map(params![now as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn delete_web_session(&self, session_id: &str) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM web_sessions WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub fn purge_web_sessions(&self, now: u64) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM web_sessions WHERE expires_at <= ?1",
            params![now as i64],
        )?;
        Ok(())
    }

    pub fn add_web_audit(
        &self,
        guild_id: serenity::GuildId,
        actor_id: serenity::UserId,
        actor_name: &str,
        action: &str,
        detail: &str,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        let created_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        conn.execute(
            "INSERT INTO web_audit_log
             (guild_id, actor_id, actor_name, action, detail, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                guild_id.get().to_string(),
                actor_id.get().to_string(),
                actor_name,
                action,
                detail,
                created_at
            ],
        )?;
        Ok(())
    }

    pub fn web_audit_log(
        &self,
        guild_id: serenity::GuildId,
        limit: usize,
    ) -> Result<Vec<AuditEntry>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT actor_name, action, detail, created_at
             FROM web_audit_log
             WHERE guild_id = ?1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![guild_id.get().to_string(), limit as i64], |row| {
            Ok(AuditEntry {
                actor_name: row.get(0)?,
                action: row.get(1)?,
                detail: row.get(2)?,
                created_at: row.get::<_, i64>(3)?.max(0) as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn add_web_feedback(
        &self,
        user_id: serenity::UserId,
        user_name: &str,
        category: &str,
        subject: &str,
        message: &str,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        let created_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        conn.execute(
            "INSERT INTO web_feedback
             (user_id, user_name, category, subject, message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                user_id.get().to_string(),
                user_name,
                category,
                subject,
                message,
                created_at
            ],
        )?;
        Ok(())
    }

    pub fn web_feedback(
        &self,
        status: Option<&str>,
        category: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<FeedbackEntry>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, user_id, user_name, category, subject, message, status, created_at
             FROM web_feedback
             WHERE (?1 IS NULL OR status = ?1)
               AND (?2 IS NULL OR category = ?2)
             ORDER BY created_at DESC, id DESC
             LIMIT ?3 OFFSET ?4",
        )?;
        let rows = stmt.query_map(
            params![status, category, limit as i64, offset as i64],
            |row| {
                Ok(FeedbackEntry {
                    id: row.get(0)?,
                    user_id: row.get::<_, String>(1)?.parse::<u64>().unwrap_or_default(),
                    user_name: row.get(2)?,
                    category: row.get(3)?,
                    subject: row.get(4)?,
                    message: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get::<_, i64>(7)?.max(0) as u64,
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn web_feedback_count(
        &self,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<usize, Error> {
        let conn = self.connect()?;
        let count = conn.query_row(
            "SELECT COUNT(*) FROM web_feedback
             WHERE (?1 IS NULL OR status = ?1)
               AND (?2 IS NULL OR category = ?2)",
            params![status, category],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn set_web_feedback_status(&self, id: i64, status: &str) -> Result<bool, Error> {
        let conn = self.connect()?;
        Ok(conn.execute(
            "UPDATE web_feedback SET status = ?2 WHERE id = ?1",
            params![id, status],
        )? > 0)
    }

    pub fn delete_web_feedback(&self, id: i64) -> Result<bool, Error> {
        let conn = self.connect()?;
        Ok(conn.execute("DELETE FROM web_feedback WHERE id = ?1", params![id])? > 0)
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
            .unwrap_or(1)
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

    pub fn replace_dj_roles(
        &self,
        guild_id: serenity::GuildId,
        role_ids: &[serenity::RoleId],
    ) -> Result<(), Error> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let guild_id = guild_id.get().to_string();
        tx.execute(
            "DELETE FROM dj_roles WHERE guild_id = ?1",
            params![guild_id],
        )?;
        for role_id in role_ids {
            tx.execute(
                "INSERT INTO dj_roles (guild_id, role_id) VALUES (?1, ?2)",
                params![guild_id, role_id.get().to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn allowed_channels(
        &self,
        guild_id: serenity::GuildId,
    ) -> Result<Vec<serenity::ChannelId>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT channel_id
            FROM allowed_channels
            WHERE guild_id = ?1
            ORDER BY channel_id ASC
            ",
        )?;

        let rows = stmt.query_map(params![guild_id.get().to_string()], |row| {
            let raw: String = row.get(0)?;
            Ok(raw.parse::<u64>().ok().map(serenity::ChannelId::new))
        })?;

        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn add_allowed_channel(
        &self,
        guild_id: serenity::GuildId,
        channel_id: serenity::ChannelId,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT OR IGNORE INTO allowed_channels (guild_id, channel_id)
            VALUES (?1, ?2)
            ",
            params![guild_id.get().to_string(), channel_id.get().to_string()],
        )?;

        Ok(())
    }

    pub fn remove_allowed_channel(
        &self,
        guild_id: serenity::GuildId,
        channel_id: serenity::ChannelId,
    ) -> Result<bool, Error> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM allowed_channels WHERE guild_id = ?1 AND channel_id = ?2",
            params![guild_id.get().to_string(), channel_id.get().to_string()],
        )?;

        Ok(changed > 0)
    }

    pub fn replace_allowed_channels(
        &self,
        guild_id: serenity::GuildId,
        channel_ids: &[serenity::ChannelId],
    ) -> Result<(), Error> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let guild_id = guild_id.get().to_string();
        tx.execute(
            "DELETE FROM allowed_channels WHERE guild_id = ?1",
            params![guild_id],
        )?;
        for channel_id in channel_ids {
            tx.execute(
                "INSERT INTO allowed_channels (guild_id, channel_id) VALUES (?1, ?2)",
                params![guild_id, channel_id.get().to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn blocked_terms(&self, guild_id: serenity::GuildId) -> Result<Vec<String>, Error> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "
            SELECT term
            FROM blocked_terms
            WHERE guild_id = ?1
            ORDER BY term ASC
            ",
        )?;

        let rows = stmt.query_map(params![guild_id.get().to_string()], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn add_blocked_term(&self, guild_id: serenity::GuildId, term: &str) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            INSERT OR IGNORE INTO blocked_terms (guild_id, term)
            VALUES (?1, ?2)
            ",
            params![guild_id.get().to_string(), normalize_term(term)],
        )?;

        Ok(())
    }

    pub fn remove_blocked_term(
        &self,
        guild_id: serenity::GuildId,
        term: &str,
    ) -> Result<bool, Error> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM blocked_terms WHERE guild_id = ?1 AND term = ?2",
            params![guild_id.get().to_string(), normalize_term(term)],
        )?;

        Ok(changed > 0)
    }

    pub fn replace_blocked_terms(
        &self,
        guild_id: serenity::GuildId,
        terms: &[String],
    ) -> Result<(), Error> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let guild_id = guild_id.get().to_string();
        tx.execute(
            "DELETE FROM blocked_terms WHERE guild_id = ?1",
            params![guild_id],
        )?;
        for term in terms {
            let term = normalize_term(term);
            if !term.is_empty() {
                tx.execute(
                    "INSERT OR IGNORE INTO blocked_terms (guild_id, term) VALUES (?1, ?2)",
                    params![guild_id, term],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn is_blocked_query(
        &self,
        guild_id: serenity::GuildId,
        query: &str,
    ) -> Result<bool, Error> {
        let query = query.to_lowercase();
        Ok(self
            .blocked_terms(guild_id)?
            .iter()
            .any(|term| !term.is_empty() && query.contains(term)))
    }

    pub fn play_cooldown_secs(&self, guild_id: serenity::GuildId) -> Result<u64, Error> {
        Ok(self
            .guild_setting_i64(guild_id, "play_cooldown_secs", 10)?
            .clamp(0, 300) as u64)
    }

    pub fn set_play_cooldown_secs(
        &self,
        guild_id: serenity::GuildId,
        seconds: u64,
    ) -> Result<(), Error> {
        self.set_guild_setting_i64(guild_id, "play_cooldown_secs", seconds.min(300) as i64)
    }

    pub fn max_queue_per_user(&self, guild_id: serenity::GuildId) -> Result<usize, Error> {
        Ok(self
            .guild_setting_i64(guild_id, "max_queue_per_user", 10)?
            .clamp(1, 100) as usize)
    }

    pub fn set_max_queue_per_user(
        &self,
        guild_id: serenity::GuildId,
        limit: usize,
    ) -> Result<(), Error> {
        self.set_guild_setting_i64(guild_id, "max_queue_per_user", limit.clamp(1, 100) as i64)
    }

    pub fn vote_skip_percent(&self, guild_id: serenity::GuildId) -> Result<u8, Error> {
        Ok(self
            .guild_setting_i64(guild_id, "vote_skip_percent", 50)?
            .clamp(1, 100) as u8)
    }

    pub fn set_vote_skip_percent(
        &self,
        guild_id: serenity::GuildId,
        percent: u8,
    ) -> Result<(), Error> {
        self.set_guild_setting_i64(guild_id, "vote_skip_percent", percent.clamp(1, 100) as i64)
    }

    pub fn normalize_cap_percent(&self, guild_id: serenity::GuildId) -> Result<u8, Error> {
        Ok(self
            .guild_setting_i64(guild_id, "normalize_cap_percent", 85)?
            .clamp(1, 200) as u8)
    }

    pub fn set_normalize_cap_percent(
        &self,
        guild_id: serenity::GuildId,
        percent: u8,
    ) -> Result<(), Error> {
        self.set_guild_setting_i64(
            guild_id,
            "normalize_cap_percent",
            percent.clamp(1, 200) as i64,
        )
    }

    pub fn idle_timeout_secs(&self, guild_id: serenity::GuildId) -> Result<u64, Error> {
        Ok(self
            .guild_setting_i64(guild_id, "idle_timeout_secs", 60)?
            .clamp(10, 600) as u64)
    }

    pub fn set_idle_timeout_secs(
        &self,
        guild_id: serenity::GuildId,
        seconds: u64,
    ) -> Result<(), Error> {
        self.set_guild_setting_i64(guild_id, "idle_timeout_secs", seconds.clamp(10, 600) as i64)
    }

    pub fn reset_guild_settings(&self, guild_id: serenity::GuildId) -> Result<(), Error> {
        let conn = self.connect()?;
        conn.execute(
            "
            UPDATE guild_settings
            SET volume_percent = 100,
                autoplay_enabled = 0,
                normalize_enabled = 1,
                play_cooldown_secs = 10,
                max_queue_per_user = 10,
                vote_skip_percent = 50,
                normalize_cap_percent = 85,
                idle_timeout_secs = 60
            WHERE guild_id = ?1
            ",
            params![guild_id.get().to_string()],
        )?;

        Ok(())
    }

    fn guild_setting_i64(
        &self,
        guild_id: serenity::GuildId,
        column: &str,
        default: i64,
    ) -> Result<i64, Error> {
        let conn = self.connect()?;
        let query = format!("SELECT {column} FROM guild_settings WHERE guild_id = ?1");
        Ok(conn
            .query_row(&query, params![guild_id.get().to_string()], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .unwrap_or(default))
    }

    fn set_guild_setting_i64(
        &self,
        guild_id: serenity::GuildId,
        column: &str,
        value: i64,
    ) -> Result<(), Error> {
        let conn = self.connect()?;
        let query = format!(
            "
            INSERT INTO guild_settings (guild_id, {column})
            VALUES (?1, ?2)
            ON CONFLICT(guild_id) DO UPDATE SET {column} = excluded.{column}
            "
        );
        conn.execute(&query, params![guild_id.get().to_string(), value])?;

        Ok(())
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

        conn.execute(
            "
            INSERT INTO user_track_stats (guild_id, user_id, tracks_played)
            VALUES (?1, ?2, 1)
            ON CONFLICT(guild_id, user_id) DO UPDATE SET
                tracks_played = user_track_stats.tracks_played + 1
            ",
            params![
                guild_id.get().to_string(),
                track.requested_by.get().to_string()
            ],
        )?;

        Ok(())
    }

    pub fn user_stats(
        &self,
        guild_id: serenity::GuildId,
        user_id: serenity::UserId,
    ) -> Result<UserStats, Error> {
        let conn = self.connect()?;
        let tracks_played = conn
            .query_row(
                "
                SELECT tracks_played
                FROM user_track_stats
                WHERE guild_id = ?1 AND user_id = ?2
                ",
                params![guild_id.get().to_string(), user_id.get().to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            .max(0) as usize;

        Ok(UserStats {
            user_id,
            tracks_played,
        })
    }

    pub fn server_stats(&self, guild_id: serenity::GuildId) -> Result<ServerStats, Error> {
        let conn = self.connect()?;
        let (unique_tracks, total_plays) = conn.query_row(
            "
            SELECT COUNT(*), COALESCE(SUM(play_count), 0)
            FROM track_history
            WHERE guild_id = ?1
            ",
            params![guild_id.get().to_string()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;

        let playlists = conn.query_row(
            "SELECT COUNT(*) FROM playlists WHERE guild_id = ?1",
            params![guild_id.get().to_string()],
            |row| row.get::<_, i64>(0),
        )?;

        Ok(ServerStats {
            unique_tracks: unique_tracks.max(0) as usize,
            total_plays: total_plays.max(0) as usize,
            playlists: playlists.max(0) as usize,
        })
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

    pub fn load_all_queues(&self) -> Result<RestoredQueues, Error> {
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

        let mut queues = RestoredQueues::new();

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

    pub fn append_playlist(
        &self,
        guild_id: serenity::GuildId,
        name: &str,
        created_by: serenity::UserId,
        tracks: &[Track],
    ) -> Result<usize, Error> {
        let mut existing = self.load_playlist(guild_id, name, created_by)?;
        existing.extend_from_slice(tracks);
        self.save_playlist(guild_id, name, created_by, &existing)?;
        Ok(existing.len())
    }

    pub fn create_empty_playlist(
        &self,
        guild_id: serenity::GuildId,
        name: &str,
        created_by: serenity::UserId,
    ) -> Result<bool, Error> {
        let conn = self.connect()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO playlists (guild_id, name, created_by, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                guild_id.get().to_string(),
                name,
                created_by.get().to_string(),
                now
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn remove_playlist_track(
        &self,
        guild_id: serenity::GuildId,
        name: &str,
        position: usize,
        requested_by: serenity::UserId,
    ) -> Result<Option<Track>, Error> {
        if position == 0 {
            return Ok(None);
        }
        let mut tracks = self.load_playlist(guild_id, name, requested_by)?;
        if position > tracks.len() {
            return Ok(None);
        }
        let removed = tracks.remove(position - 1);
        self.save_playlist(guild_id, name, requested_by, &tracks)?;
        Ok(Some(removed))
    }

    pub fn move_playlist_track(
        &self,
        guild_id: serenity::GuildId,
        name: &str,
        from_position: usize,
        to_position: usize,
        requested_by: serenity::UserId,
    ) -> Result<bool, Error> {
        if from_position == 0 || to_position == 0 {
            return Ok(false);
        }
        let mut tracks = self.load_playlist(guild_id, name, requested_by)?;
        if from_position > tracks.len() || to_position > tracks.len() {
            return Ok(false);
        }
        let track = tracks.remove(from_position - 1);
        tracks.insert(to_position - 1, track);
        self.save_playlist(guild_id, name, requested_by, &tracks)?;
        Ok(true)
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

    pub fn playlist_exists(&self, guild_id: serenity::GuildId, name: &str) -> Result<bool, Error> {
        let conn = self.connect()?;
        Ok(conn
            .query_row(
                "SELECT 1 FROM playlists WHERE guild_id = ?1 AND name = ?2",
                params![guild_id.get().to_string(), name],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn search_playlists(
        &self,
        guild_id: serenity::GuildId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PlaylistSummary>, Error> {
        let conn = self.connect()?;
        let pattern = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "
            SELECT playlists.name, COUNT(playlist_tracks.position) AS track_count
            FROM playlists
            LEFT JOIN playlist_tracks
                ON playlist_tracks.guild_id = playlists.guild_id
                AND playlist_tracks.playlist_name = playlists.name
            WHERE playlists.guild_id = ?1
                AND (?2 = '%%' OR playlists.name LIKE ?2)
            GROUP BY playlists.name
            ORDER BY playlists.name ASC
            LIMIT ?3
            ",
        )?;

        let rows = stmt.query_map(
            params![guild_id.get().to_string(), pattern, limit as i64],
            |row| {
                Ok(PlaylistSummary {
                    name: row.get(0)?,
                    track_count: row.get::<_, i64>(1)?.max(0) as usize,
                })
            },
        )?;

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

    pub fn rename_playlist(
        &self,
        guild_id: serenity::GuildId,
        old_name: &str,
        new_name: &str,
    ) -> Result<bool, Error> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
        let guild_id_raw = guild_id.get().to_string();

        let changed = tx.execute(
            "
            UPDATE playlists
            SET name = ?3
            WHERE guild_id = ?1 AND name = ?2
            ",
            params![guild_id_raw, old_name, new_name],
        )?;

        if changed > 0 {
            tx.execute(
                "
                UPDATE playlist_tracks
                SET playlist_name = ?3
                WHERE guild_id = ?1 AND playlist_name = ?2
                ",
                params![guild_id_raw, old_name, new_name],
            )?;
        }

        tx.commit()?;
        Ok(changed > 0)
    }
}

fn normalize_term(term: &str) -> String {
    term.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_rename_and_audit_are_persistent() {
        let path = std::env::temp_dir().join(format!("music-bot-test-{}.db", uuid::Uuid::new_v4()));
        let db = Database::new(&path).expect("database opens");
        let guild_id = serenity::GuildId::new(10);
        let user_id = serenity::UserId::new(20);
        let conn = db.connect().expect("configured connection opens");
        assert_eq!(
            conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .expect("journal mode reads")
                .to_lowercase(),
            "wal"
        );
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .expect("foreign keys read"),
            1
        );
        drop(conn);
        let track = Track {
            title: "Track".to_string(),
            url: "https://example.com/track".to_string(),
            duration_secs: Some(60),
            requested_by: user_id,
            thumbnail: None,
        };
        db.save_playlist(guild_id, "Old", user_id, std::slice::from_ref(&track))
            .expect("playlist saves");
        assert!(db
            .rename_playlist(guild_id, "Old", "New")
            .expect("playlist renames"));
        assert_eq!(
            db.load_playlist(guild_id, "New", user_id)
                .expect("playlist loads")
                .len(),
            1
        );
        assert!(db
            .create_empty_playlist(guild_id, "Manual", user_id)
            .expect("empty playlist creates"));
        assert!(!db
            .create_empty_playlist(guild_id, "Manual", user_id)
            .expect("duplicate playlist is ignored"));
        assert!(db
            .playlist_exists(guild_id, "Manual")
            .expect("playlist exists"));

        let second = Track {
            title: "Second".to_string(),
            url: "https://example.com/second".to_string(),
            duration_secs: Some(90),
            requested_by: user_id,
            thumbnail: None,
        };
        db.append_playlist(guild_id, "New", user_id, &[second])
            .expect("track appends");
        assert!(db
            .move_playlist_track(guild_id, "New", 2, 1, user_id)
            .expect("track moves"));
        let reordered = db
            .load_playlist(guild_id, "New", user_id)
            .expect("reordered playlist loads");
        assert_eq!(reordered[0].title, "Second");
        assert_eq!(
            db.remove_playlist_track(guild_id, "New", 2, user_id)
                .expect("track removes")
                .expect("track was present")
                .title,
            track.title
        );

        db.add_web_audit(guild_id, user_id, "Admin", "playlist.renamed", "Old to New")
            .expect("audit saves");
        let audit = db.web_audit_log(guild_id, 10).expect("audit loads");
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].action, "playlist.renamed");

        db.add_web_feedback(
            user_id,
            "Listener",
            "bug",
            "Skip issue",
            "Song played twice",
        )
        .expect("feedback saves");
        let feedback = db
            .web_feedback(Some("open"), Some("bug"), 10, 0)
            .expect("feedback loads");
        assert_eq!(feedback.len(), 1);
        assert_eq!(feedback[0].category, "bug");
        assert_eq!(feedback[0].subject, "Skip issue");
        assert_eq!(feedback[0].status, "open");
        assert!(db
            .set_web_feedback_status(feedback[0].id, "resolved")
            .expect("feedback resolves"));
        assert_eq!(
            db.web_feedback_count(Some("resolved"), None)
                .expect("resolved feedback counts"),
            1
        );

        let backup_dir =
            std::env::temp_dir().join(format!("music-bot-backup-test-{}", uuid::Uuid::new_v4()));
        let backup_path = db
            .create_backup(&backup_dir, 2)
            .expect("online backup succeeds");
        let backup_db = Database::new(&backup_path).expect("backup database opens");
        assert!(backup_db
            .playlist_exists(guild_id, "New")
            .expect("backup contains playlist"));
        drop(backup_db);

        drop(db);
        std::fs::remove_file(path).expect("test database removed");
        std::fs::remove_file(backup_path).expect("test backup removed");
        std::fs::remove_dir(backup_dir).expect("test backup directory removed");
    }
}
