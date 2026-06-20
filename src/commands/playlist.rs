use poise::serenity_prelude as serenity;
use serde_json::Value;
use std::{process::Command, time::Duration};

use crate::{
    music::{player, track::Track},
    permissions,
    ui::{player_panel, queue_panel},
    Ctx, Error,
};

/// Kelola saved playlist server ini.
#[poise::command(
    slash_command,
    subcommands("save", "append", "load", "import_youtube", "list", "rename", "delete"),
    subcommand_required
)]
pub async fn playlist(_ctx: Ctx<'_>) -> Result<(), Error> {
    Ok(())
}

pub async fn autocomplete_playlist(
    ctx: Ctx<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let Some(guild_id) = ctx.guild_id() else {
        return Vec::new();
    };

    match ctx.data().db.search_playlists(guild_id, partial, 10) {
        Ok(playlists) => playlists
            .into_iter()
            .map(|playlist| {
                serenity::AutocompleteChoice::new(
                    format!("{} [{} track(s)]", playlist.name, playlist.track_count),
                    playlist.name,
                )
            })
            .collect(),
        Err(err) => {
            tracing::warn!("playlist autocomplete failed: {err:?}");
            Vec::new()
        }
    }
}

/// Simpan now playing + queue sebagai playlist.
#[poise::command(slash_command)]
pub async fn save(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"] name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let name = normalize_name(&name)?;

    let tracks = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let state = state_lock.lock().await;
        collect_playlist_tracks(&state.now_playing, &state.queue)
    };

    if tracks.is_empty() {
        ctx.say("Tidak ada lagu buat disimpan.").await?;
        return Ok(());
    }

    ctx.data()
        .db
        .save_playlist(guild_id, &name, ctx.author().id, &tracks)?;

    ctx.say(format!(
        "Saved playlist `{name}` with `{}` track(s).",
        tracks.len()
    ))
    .await?;

    Ok(())
}

/// Tambahkan now playing + queue ke playlist yang sudah ada.
#[poise::command(slash_command)]
pub async fn append(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"]
    #[autocomplete = "autocomplete_playlist"]
    name: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let name = normalize_name(&name)?;

    let tracks = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let state = state_lock.lock().await;
        collect_playlist_tracks(&state.now_playing, &state.queue)
    };

    if tracks.is_empty() {
        ctx.say("Tidak ada lagu buat ditambahkan.").await?;
        return Ok(());
    }

    let total = ctx
        .data()
        .db
        .append_playlist(guild_id, &name, ctx.author().id, &tracks)?;

    ctx.say(format!(
        "Appended `{}` track(s) to `{name}`. Playlist now has `{total}` track(s).",
        tracks.len()
    ))
    .await?;

    Ok(())
}

/// Load playlist ke queue.
#[poise::command(slash_command)]
pub async fn load(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"]
    #[autocomplete = "autocomplete_playlist"]
    name: String,
    #[description = "Mode: append, replace, atau playnow"] mode: Option<String>,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let channel_id = ctx.channel_id();
    let user_id = ctx.author().id;
    let name = normalize_name(&name)?;
    let mode = LoadMode::parse(mode.as_deref())?;
    let tracks = ctx
        .data()
        .db
        .load_playlist(guild_id, &name, ctx.author().id)?;

    if tracks.is_empty() {
        ctx.say(format!("Playlist `{name}` kosong atau tidak ditemukan."))
            .await?;
        return Ok(());
    }

    let tracks = apply_user_queue_limit(ctx, guild_id, user_id, tracks, mode).await?;
    if tracks.is_empty() {
        return Ok(());
    }

    if let Err(err) = player::join_user_channel(ctx.serenity_context(), guild_id, user_id).await {
        ctx.say(format!("Gagal join voice channel: {err}"))
            .await
            .ok();
        return Ok(());
    }

    load_tracks_into_state(ctx, guild_id, channel_id, user_id, mode, tracks.clone()).await?;
    player::persist_queue(ctx.data(), guild_id).await;

    player::start_if_idle(ctx.serenity_context(), ctx.data(), guild_id).await?;
    player_panel::send_or_update_player_panel(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        channel_id,
    )
    .await
    .ok();
    queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
        .await
        .ok();

    ctx.say(format!(
        "Loaded playlist `{name}` with `{}` track(s).",
        tracks.len()
    ))
    .await?;

    Ok(())
}

/// Import playlist YouTube menjadi saved playlist.
#[poise::command(slash_command, rename = "import-youtube")]
pub async fn import_youtube(
    ctx: Ctx<'_>,
    #[description = "Nama saved playlist"] name: String,
    #[description = "URL playlist YouTube"] url: String,
    #[description = "Tambahkan ke playlist jika sudah ada"] append: Option<bool>,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let name = normalize_name(&name)?;
    validate_youtube_playlist_url(&url)?;
    ctx.defer_ephemeral().await?;

    let requested_by = ctx.author().id;
    let import_url = url.clone();
    let tracks = tokio::time::timeout(
        Duration::from_secs(45),
        tokio::task::spawn_blocking(move || import_youtube_tracks(&import_url, requested_by)),
    )
    .await
    .map_err(|_| "Import playlist timeout setelah 45 detik.")??;
    let tracks = tracks?;

    if tracks.is_empty() {
        ctx.say("Tidak ada video yang bisa diimport dari playlist itu.")
            .await?;
        return Ok(());
    }

    let imported = tracks.len();
    let total = if append.unwrap_or(false) {
        ctx.data()
            .db
            .append_playlist(guild_id, &name, requested_by, &tracks)?
    } else {
        ctx.data()
            .db
            .save_playlist(guild_id, &name, requested_by, &tracks)?;
        imported
    };

    ctx.say(format!(
        "Imported `{imported}` track(s) dari YouTube ke playlist `{name}`. Total sekarang: `{total}` track(s)."
    ))
    .await?;

    Ok(())
}

/// Lihat semua saved playlist.
#[poise::command(slash_command)]
pub async fn list(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let playlists = ctx.data().db.list_playlists(guild_id)?;

    if playlists.is_empty() {
        ctx.say("Belum ada saved playlist di server ini.").await?;
        return Ok(());
    }

    let desc = playlists
        .iter()
        .map(|playlist| {
            format!(
                "- `{}` - `{}` track(s)",
                playlist.name, playlist.track_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    ctx.say(format!("Saved playlists:\n{desc}")).await?;

    Ok(())
}

/// Rename saved playlist.
#[poise::command(slash_command)]
pub async fn rename(
    ctx: Ctx<'_>,
    #[description = "Nama playlist lama"]
    #[autocomplete = "autocomplete_playlist"]
    old_name: String,
    #[description = "Nama playlist baru"] new_name: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let old_name = normalize_name(&old_name)?;
    let new_name = normalize_name(&new_name)?;

    if ctx
        .data()
        .db
        .rename_playlist(guild_id, &old_name, &new_name)?
    {
        ctx.say(format!("Renamed playlist `{old_name}` to `{new_name}`."))
            .await?;
    } else {
        ctx.say(format!("Playlist `{old_name}` tidak ditemukan."))
            .await?;
    }

    Ok(())
}

/// Hapus saved playlist.
#[poise::command(slash_command)]
pub async fn delete(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"]
    #[autocomplete = "autocomplete_playlist"]
    name: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let name = normalize_name(&name)?;

    if ctx.data().db.delete_playlist(guild_id, &name)? {
        ctx.say(format!("Deleted playlist `{name}`.")).await?;
    } else {
        ctx.say(format!("Playlist `{name}` tidak ditemukan."))
            .await?;
    }

    Ok(())
}

fn collect_playlist_tracks(
    now_playing: &Option<Track>,
    queue: &std::collections::VecDeque<Track>,
) -> Vec<Track> {
    now_playing
        .iter()
        .chain(queue.iter())
        .cloned()
        .collect::<Vec<_>>()
}

fn normalize_name(name: &str) -> Result<String, Error> {
    let name = name.trim();

    if name.is_empty() {
        return Err("Nama playlist tidak boleh kosong.".into());
    }

    if name.len() > 64 {
        return Err("Nama playlist maksimal 64 karakter.".into());
    }

    Ok(name.to_string())
}

fn validate_youtube_playlist_url(raw: &str) -> Result<(), Error> {
    let parsed = url::Url::parse(raw.trim()).map_err(|_| "URL playlist tidak valid.")?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let is_youtube = matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be"
    );
    let has_playlist_id = parsed
        .query_pairs()
        .any(|(key, value)| key == "list" && !value.is_empty());

    if !is_youtube || !has_playlist_id {
        return Err("Masukkan URL playlist YouTube yang punya parameter `list=`.".into());
    }

    Ok(())
}

fn import_youtube_tracks(url: &str, requested_by: serenity::UserId) -> Result<Vec<Track>, Error> {
    let output = Command::new("yt-dlp")
        .args([
            "--flat-playlist",
            "--playlist-end",
            "100",
            "--dump-json",
            "--skip-download",
            "--no-warnings",
            url,
        ])
        .output()
        .map_err(|err| format!("Gagal menjalankan yt-dlp: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp gagal import playlist: {}", stderr.trim()).into());
    }

    let tracks = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|item| youtube_item_to_track(&item, requested_by))
        .collect::<Vec<_>>();

    Ok(tracks)
}

fn youtube_item_to_track(item: &Value, requested_by: serenity::UserId) -> Option<Track> {
    let id = item.get("id")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }

    let title = item
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(id)
        .to_string();
    let webpage_url = item
        .get("webpage_url")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("http"))
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}"));

    Some(Track {
        title,
        url: webpage_url,
        duration_secs: item
            .get("duration")
            .and_then(Value::as_f64)
            .map(|value| value.round() as u64),
        requested_by,
        thumbnail: item
            .get("thumbnail")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadMode {
    Append,
    Replace,
    PlayNow,
}

impl LoadMode {
    fn parse(raw: Option<&str>) -> Result<Self, Error> {
        match raw.unwrap_or("append").trim().to_lowercase().as_str() {
            "" | "append" => Ok(Self::Append),
            "replace" => Ok(Self::Replace),
            "playnow" | "play-now" | "now" => Ok(Self::PlayNow),
            _ => Err("Mode playlist harus `append`, `replace`, atau `playnow`.".into()),
        }
    }
}

async fn apply_user_queue_limit(
    ctx: Ctx<'_>,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
    tracks: Vec<Track>,
    mode: LoadMode,
) -> Result<Vec<Track>, Error> {
    if mode == LoadMode::Replace {
        return Ok(tracks);
    }

    let max_queue_per_user = ctx.data().db.max_queue_per_user(guild_id)?;
    let queued_by_user = player::user_queue_count(ctx.data(), guild_id, user_id).await;
    let remaining_slots = max_queue_per_user.saturating_sub(queued_by_user);
    if remaining_slots == 0 {
        ctx.say(format!(
            "Queue lu sudah mencapai batas `{max_queue_per_user}` lagu. Hapus beberapa dulu sebelum load playlist."
        ))
        .await?;
        return Ok(Vec::new());
    }

    Ok(tracks.into_iter().take(remaining_slots).collect())
}

async fn load_tracks_into_state(
    ctx: Ctx<'_>,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
    _user_id: serenity::UserId,
    mode: LoadMode,
    tracks: Vec<Track>,
) -> Result<(), Error> {
    let state_lock = ctx.data().music.get(guild_id).await;
    let mut state = state_lock.lock().await;
    state.player_channel_id = Some(channel_id);

    match mode {
        LoadMode::Append => state.queue.extend(tracks),
        LoadMode::Replace => {
            state.queue.clear();
            state.queue.extend(tracks);
        }
        LoadMode::PlayNow => {
            let mut tracks = tracks.into_iter();
            if let Some(first) = tracks.next() {
                state.queue.extend(tracks);
                drop(state);
                player::play_now(ctx.serenity_context(), ctx.data(), guild_id, first).await?;
            }
        }
    }

    Ok(())
}
