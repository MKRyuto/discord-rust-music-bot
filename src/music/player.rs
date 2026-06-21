use std::{
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use poise::serenity_prelude as serenity;
use songbird::input::{ChildContainer, Input, YoutubeDl};
use songbird::tracks::{ReadyState, TrackHandle};
use songbird::{Event, EventContext, EventHandler, TrackEvent};

use crate::music::track::Track;
use crate::ui::{player_panel, queue_panel};
use crate::{Data, Error};

fn is_http_url(input: &str) -> bool {
    url::Url::parse(input)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

fn youtube_dl_for(data: &Data, query_or_url: String) -> YoutubeDl<'static> {
    if is_http_url(&query_or_url) {
        YoutubeDl::new(data.http_client.clone(), query_or_url)
    } else {
        YoutubeDl::new_search(data.http_client.clone(), query_or_url)
    }
}

fn playback_input(
    data: &Data,
    guild_id: serenity::GuildId,
    source: &str,
    start_at: Option<Duration>,
) -> Result<Input, Error> {
    let normalize = data.db.normalize_enabled(guild_id)?;
    if !normalize {
        return Ok(youtube_dl_for(data, source.to_string()).into());
    }

    let ytdl_source = if is_http_url(source) {
        source.to_string()
    } else {
        format!("ytsearch1:{source}")
    };
    let mut downloader = Command::new("yt-dlp")
        .args([
            "-f",
            "ba[abr>0][vcodec=none]/best",
            "--no-playlist",
            "--no-warnings",
            "-o",
            "-",
            &ytdl_source,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Gagal menjalankan yt-dlp untuk audio pipeline: {err}"))?;
    let downloader_stdout = downloader
        .stdout
        .take()
        .ok_or("Gagal membuka stream yt-dlp.")?;

    let filter = audio_filter();
    let mut ffmpeg_args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
    ];
    if let Some(position) = start_at.filter(|position| !position.is_zero()) {
        ffmpeg_args.push("-ss".to_string());
        ffmpeg_args.push(format!("{:.3}", position.as_secs_f64()));
    }
    ffmpeg_args.extend([
        "-i".to_string(),
        "pipe:0".to_string(),
        "-vn".to_string(),
        "-af".to_string(),
        filter.clone(),
        "-ac".to_string(),
        "2".to_string(),
        "-ar".to_string(),
        "48000".to_string(),
        "-f".to_string(),
        "wav".to_string(),
        "pipe:1".to_string(),
    ]);

    let processor = Command::new("ffmpeg")
        .args(ffmpeg_args)
        .stdin(Stdio::from(downloader_stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Gagal menjalankan FFmpeg audio pipeline: {err}"))?;

    tracing::info!(filter, "using FFmpeg loudness normalization pipeline");
    Ok(ChildContainer::new(vec![downloader, processor]).into())
}

fn audio_filter() -> String {
    "loudnorm=I=-16:LRA=11:TP=-1.5,dynaudnorm=f=250:g=15:p=0.9:m=8".to_string()
}

pub async fn ensure_guild_settings(data: &Data, guild_id: serenity::GuildId) -> Result<(), Error> {
    let should_load = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        !state.volume_loaded
    };

    if should_load {
        let volume_percent = data.db.guild_volume(guild_id)?;
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.volume_percent = volume_percent;
        state.volume_loaded = true;
    }

    Ok(())
}

pub async fn set_volume(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    volume_percent: u8,
) -> Result<(), Error> {
    set_volume_from_dashboard(data, guild_id, volume_percent).await?;

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();

    Ok(())
}

pub async fn set_volume_from_dashboard(
    data: &Data,
    guild_id: serenity::GuildId,
    volume_percent: u8,
) -> Result<(), Error> {
    data.db.set_guild_volume(guild_id, volume_percent)?;

    let current_handle = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.volume_percent = volume_percent;
        state.volume_loaded = true;
        state.current_handle.clone()
    };

    if let Some(handle) = current_handle {
        handle
            .set_volume(effective_volume_percent(data, guild_id, volume_percent)? as f32 / 100.0)?;
    }

    Ok(())
}

fn effective_volume_percent(
    data: &Data,
    guild_id: serenity::GuildId,
    volume_percent: u8,
) -> Result<u8, Error> {
    if data.db.normalize_enabled(guild_id)? {
        Ok(volume_percent.min(data.db.normalize_cap_percent(guild_id)?))
    } else {
        Ok(volume_percent)
    }
}

pub async fn adjust_volume(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    delta: i16,
) -> Result<u8, Error> {
    ensure_guild_settings(data, guild_id).await?;

    let next_volume = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        (state.volume_percent as i16 + delta).clamp(0, 200) as u8
    };

    set_volume(ctx, data, guild_id, next_volume).await?;
    Ok(next_volume)
}

pub async fn shuffle_queue(data: &Data, guild_id: serenity::GuildId) -> usize {
    let state_lock = data.music.get(guild_id).await;
    let mut state = state_lock.lock().await;

    if state.queue.len() < 2 {
        return 0;
    }

    let mut tracks = state.queue.drain(..).collect::<Vec<_>>();
    shuffle_tracks(&mut tracks);
    let total = tracks.len();
    state.queue = tracks.into();
    state.queue_page = 0;
    drop(state);

    persist_queue(data, guild_id).await;
    total
}

fn shuffle_tracks<T>(tracks: &mut [T]) {
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15);

    for idx in (1..tracks.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let swap_idx = (seed as usize) % (idx + 1);
        tracks.swap(idx, swap_idx);
    }
}

pub async fn persist_queue(data: &Data, guild_id: serenity::GuildId) {
    let (now_playing, queue) = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        (state.now_playing.clone(), state.queue.clone())
    };

    if let Err(err) = data.db.save_queue(guild_id, &now_playing, &queue) {
        tracing::warn!("failed to persist queue: {err:?}");
    }
}

pub async fn remove_queued_track(
    data: &Data,
    guild_id: serenity::GuildId,
    position: usize,
) -> Option<Track> {
    if position == 0 {
        return None;
    }

    let removed = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let index = position - 1;
        let removed = state.queue.remove(index);
        if removed.is_some() {
            state.queue_page = 0;
        }
        removed
    };

    if removed.is_some() {
        persist_queue(data, guild_id).await;
    }

    removed
}

pub async fn remove_queued_track_matching(
    data: &Data,
    guild_id: serenity::GuildId,
    query: &str,
) -> Option<(usize, Track)> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }

    let removed = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let index = state.queue.iter().position(|track| {
            track.title.to_lowercase().contains(&needle)
                || track.url.to_lowercase().contains(&needle)
        })?;

        let removed = state.queue.remove(index).map(|track| (index + 1, track));
        if removed.is_some() {
            state.queue_page = 0;
        }
        removed
    };

    if removed.is_some() {
        persist_queue(data, guild_id).await;
    }

    removed
}

pub async fn remove_queued_track_range(
    data: &Data,
    guild_id: serenity::GuildId,
    start_position: usize,
    end_position: usize,
) -> Vec<Track> {
    if start_position == 0 || end_position < start_position {
        return Vec::new();
    }

    let removed = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let len = state.queue.len();
        if start_position > len {
            return Vec::new();
        }

        let start = start_position - 1;
        let end = end_position.min(len);
        let removed = state.queue.drain(start..end).collect::<Vec<_>>();
        if !removed.is_empty() {
            state.queue_page = 0;
        }
        removed
    };

    if !removed.is_empty() {
        persist_queue(data, guild_id).await;
    }

    removed
}

pub async fn set_queue_page(
    data: &Data,
    guild_id: serenity::GuildId,
    page_number: usize,
) -> Option<(usize, usize)> {
    if page_number == 0 {
        return None;
    }

    let state_lock = data.music.get(guild_id).await;
    let mut state = state_lock.lock().await;
    let total_pages = state.queue.len().div_ceil(10).max(1);
    let page_index = page_number.min(total_pages) - 1;
    state.queue_page = page_index;
    Some((page_index + 1, total_pages))
}

pub async fn move_queued_track(
    data: &Data,
    guild_id: serenity::GuildId,
    from_position: usize,
    to_position: usize,
) -> bool {
    if from_position == 0 || to_position == 0 {
        return false;
    }

    let moved = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let len = state.queue.len();
        if from_position > len || to_position > len {
            return false;
        }

        let Some(track) = state.queue.remove(from_position - 1) else {
            return false;
        };

        state.queue.insert(to_position - 1, track);
        state.queue_page = 0;
        true
    };

    if moved {
        persist_queue(data, guild_id).await;
    }

    moved
}

pub async fn play_cooldown_remaining(
    data: &Data,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> Option<u64> {
    let cooldown = data
        .db
        .play_cooldown_secs(guild_id)
        .map(Duration::from_secs)
        .unwrap_or_else(|_| Duration::from_secs(10));

    if cooldown.is_zero() {
        return None;
    }

    let state_lock = data.music.get(guild_id).await;
    let mut state = state_lock.lock().await;
    let now = Instant::now();

    state
        .recent_play_requests
        .retain(|_, last| now.duration_since(*last) < cooldown);

    if let Some(last) = state.recent_play_requests.get(&user_id) {
        let elapsed = now.duration_since(*last);
        if elapsed < cooldown {
            return Some((cooldown - elapsed).as_secs().max(1));
        }
    }

    state.recent_play_requests.insert(user_id, now);
    None
}

pub async fn user_queue_count(
    data: &Data,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> usize {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    state
        .queue
        .iter()
        .filter(|track| track.requested_by == user_id)
        .count()
        + usize::from(
            state
                .now_playing
                .as_ref()
                .is_some_and(|track| track.requested_by == user_id),
        )
}

pub async fn vote_skip(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> Result<(usize, usize, bool), Error> {
    if user_voice_channel(ctx, guild_id, user_id).is_none() {
        return Err("Lu harus join voice channel dulu buat vote skip.".into());
    }

    let needed = vote_skip_threshold(ctx, data, guild_id, user_id);
    let votes = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        if state.now_playing.is_none() {
            return Err("Tidak ada lagu yang sedang diputar.".into());
        }
        state.skip_votes.insert(user_id);
        state.skip_votes.len()
    };

    if votes >= needed {
        skip(ctx, data, guild_id).await?;
        Ok((votes, needed, true))
    } else {
        player_panel::update_player_message(ctx, data, guild_id)
            .await
            .ok();
        Ok((votes, needed, false))
    }
}

fn vote_skip_threshold(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> usize {
    let Some(channel_id) = user_voice_channel(ctx, guild_id, user_id) else {
        return 1;
    };
    let Some(guild) = guild_id.to_guild_cached(ctx) else {
        return 1;
    };

    let listeners = guild
        .voice_states
        .values()
        .filter(|state| state.channel_id == Some(channel_id))
        .count();

    if listeners <= 2 {
        1
    } else {
        let percent = data.db.vote_skip_percent(guild_id).unwrap_or(50) as usize;
        ((listeners * percent).div_ceil(100)).max(1)
    }
}

/// Cari voice channel user di guild.
pub fn user_voice_channel(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> Option<serenity::ChannelId> {
    let guild = guild_id.to_guild_cached(ctx)?;
    let voice_state = guild.voice_states.get(&user_id)?;
    voice_state.channel_id
}

/// Join voice channel kalau belum join.
pub async fn join_user_channel(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
) -> Result<serenity::ChannelId, Error> {
    let channel_id =
        user_voice_channel(ctx, guild_id, user_id).ok_or("Lu harus join voice channel dulu.")?;

    ensure_voice_permissions(ctx, guild_id, channel_id).await?;

    let manager = songbird::get(ctx)
        .await
        .ok_or("Songbird voice client belum terpasang.")?
        .clone();

    match manager.join(guild_id, channel_id).await {
        Ok(_handler_lock) => {}
        Err(error) if error.should_leave_server() => {
            tracing::warn!(%guild_id, %channel_id, "Discord voice gateway timed out; retrying join");
            if let Err(remove_error) = manager.remove(guild_id).await {
                tracing::warn!(?remove_error, %guild_id, "failed to clear stale voice state");
            }
            tokio::time::sleep(Duration::from_millis(750)).await;
            manager.join(guild_id, channel_id).await.map_err(|retry_error| {
                if retry_error.should_leave_server() {
                    "Discord voice gateway tidak merespons setelah retry. Pastikan bot diizinkan Connect dan Speak, voice channel tidak dibatasi, lalu coba kick dan invite ulang bot."
                        .to_string()
                } else {
                    format!("Gagal join voice channel setelah retry: {retry_error}")
                }
            })?;
        }
        Err(error) => return Err(format!("Gagal join voice channel: {error}").into()),
    }

    Ok(channel_id)
}

async fn ensure_voice_permissions(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) -> Result<(), Error> {
    let bot_id = ctx.cache.current_user().id;
    let member = guild_id
        .member(ctx, bot_id)
        .await
        .map_err(|error| format!("Gagal memeriksa permission bot di server: {error}"))?;
    let permissions = {
        let guild = guild_id
            .to_guild_cached(ctx)
            .ok_or("Data server belum tersedia di cache. Coba lagi beberapa detik.")?;
        let channel = guild
            .channels
            .get(&channel_id)
            .ok_or("Voice channel tidak ditemukan di cache Discord.")?;
        guild.user_permissions_in(channel, &member)
    };
    let missing = missing_voice_permissions(permissions);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Bot tidak punya permission {} di voice channel ini. Periksa role bot dan channel permission overrides.",
            missing.join(" dan ")
        )
        .into())
    }
}

fn missing_voice_permissions(permissions: serenity::Permissions) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !permissions.contains(serenity::Permissions::CONNECT) {
        missing.push("Connect");
    }
    if !permissions.contains(serenity::Permissions::SPEAK) {
        missing.push("Speak");
    }
    missing
}

/// Ambil metadata sederhana dari query/url.
/// MVP: pakai YoutubeDl query(). Kalau gagal, tetap bikin Track unknown.
pub async fn resolve_track(
    data: &Data,
    query_or_url: String,
    requested_by: serenity::UserId,
) -> Track {
    let mut ytdl = youtube_dl_for(data, query_or_url.clone());

    match ytdl.query(1).await {
        Ok(mut results) => {
            if let Some(item) = results.pop() {
                let title = item.title.unwrap_or_else(|| query_or_url.clone());
                let url = item
                    .webpage_url
                    .or(Some(item.url))
                    .unwrap_or_else(|| query_or_url.clone());

                Track {
                    title,
                    url,
                    duration_secs: item.duration.map(|d| d.round() as u64),
                    requested_by,
                    thumbnail: item.thumbnail,
                }
            } else {
                Track::unknown(query_or_url, requested_by)
            }
        }
        Err(err) => {
            tracing::warn!("yt-dlp metadata failed: {err:?}");
            Track::unknown(query_or_url, requested_by)
        }
    }
}

/// Kalau tidak ada lagu berjalan, ambil queue depan dan play.
pub async fn start_if_idle(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let state_lock = data.music.get(guild_id).await;

    let next_track = {
        let mut state = state_lock.lock().await;
        if state.now_playing.is_some() {
            return Ok(());
        }
        let Some(track) = state.queue.pop_front() else {
            return Ok(());
        };
        state.now_playing = Some(track.clone());
        state.is_paused = false;
        state.skip_votes.clear();
        track
    };

    play_track(ctx, data, guild_id, next_track).await?;
    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();
    persist_queue(data, guild_id).await;

    Ok(())
}

pub async fn play_now(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    track: Track,
) -> Result<(), Error> {
    let previous_handle = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let previous_handle = state.current_handle.take();
        state.now_playing = Some(track.clone());
        state.is_paused = false;
        state.skip_votes.clear();
        previous_handle
    };

    if let Some(handle) = previous_handle {
        handle.stop().ok();
    }

    play_track(ctx, data, guild_id, track).await?;
    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();
    persist_queue(data, guild_id).await;

    Ok(())
}

pub async fn replay(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let (current_handle, track) = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let Some(track) = state.now_playing.clone() else {
            return Err("Tidak ada lagu yang sedang diputar.".into());
        };

        let current_handle = state.current_handle.take();
        state.is_paused = false;
        state.skip_votes.clear();
        (current_handle, track)
    };

    if let Some(handle) = current_handle {
        handle.stop().ok();
    }

    play_track(ctx, data, guild_id, track).await?;
    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    Ok(())
}

pub async fn previous(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let (current_handle, previous_track) = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let Some(previous_track) = state.previous_tracks.pop_back() else {
            return Err("Belum ada previous track.".into());
        };

        if let Some(current) = state.now_playing.take() {
            state.queue.push_front(current);
        }

        let current_handle = state.current_handle.take();
        state.now_playing = Some(previous_track.clone());
        state.is_paused = false;
        state.skip_votes.clear();
        (current_handle, previous_track)
    };

    if let Some(handle) = current_handle {
        handle.stop().ok();
    }

    play_track(ctx, data, guild_id, previous_track).await?;
    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    persist_queue(data, guild_id).await;

    Ok(())
}

/// Play track sekarang.
pub async fn play_track(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    track: Track,
) -> Result<(), Error> {
    play_track_at(ctx, data, guild_id, track, None).await
}

async fn play_track_at(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    track: Track,
    start_at: Option<Duration>,
) -> Result<(), Error> {
    ensure_guild_settings(data, guild_id).await?;

    let manager = songbird::get(ctx)
        .await
        .ok_or("Songbird voice client belum tersedia.")?
        .clone();

    let handler_lock = manager
        .get(guild_id)
        .ok_or("Bot belum join voice channel.")?;

    let mut handler = handler_lock.lock().await;

    tracing::info!(title = %track.title, source = %track.url, "starting track playback");

    let input = playback_input(data, guild_id, &track.url, start_at)?;
    let track_handle = handler.play_input(input);

    {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        let volume_percent = effective_volume_percent(data, guild_id, state.volume_percent)?;
        track_handle.set_volume(volume_percent as f32 / 100.0)?;
        drop(state);
        let mut state = state_lock.lock().await;
        state.current_handle = Some(track_handle.clone());
    }

    track_handle.add_event(
        Event::Track(TrackEvent::Playable),
        TrackPlayableNotifier {
            data: data.clone(),
            guild_id,
            track: track.clone(),
            record_history: start_at.is_none(),
        },
    )?;

    track_handle.add_event(
        Event::Track(TrackEvent::Error),
        TrackErrorNotifier {
            ctx: ctx.clone(),
            data: data.clone(),
            guild_id,
        },
    )?;

    track_handle.add_event(
        Event::Track(TrackEvent::End),
        TrackEndNotifier {
            ctx: ctx.clone(),
            data: data.clone(),
            guild_id,
        },
    )?;

    spawn_prepare_timeout(
        ctx.clone(),
        data.clone(),
        guild_id,
        track_handle.clone(),
        track.title.clone(),
    );

    Ok(())
}

fn spawn_prepare_timeout(
    ctx: serenity::Context,
    data: Data,
    guild_id: serenity::GuildId,
    track_handle: TrackHandle,
    title: String,
) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(25)).await;

        let Ok(state) = track_handle.get_info().await else {
            return;
        };

        if state.ready == ReadyState::Playable {
            return;
        }

        tracing::warn!(
            title = %title,
            ready = ?state.ready,
            playing = ?state.playing,
            "track prepare timed out; skipping"
        );

        notify_playback_issue(
            &ctx,
            &data,
            guild_id,
            format!("Track **{title}** stuck while preparing, skipping to the next track."),
        )
        .await;

        {
            let state_lock = data.music.get(guild_id).await;
            let mut music_state = state_lock.lock().await;
            if music_state
                .current_handle
                .as_ref()
                .is_some_and(|handle| handle.uuid() == track_handle.uuid())
            {
                music_state.current_handle = None;
            } else {
                return;
            }
        }

        track_handle.stop().ok();

        if let Err(err) = advance_queue(&ctx, &data, guild_id).await {
            tracing::warn!("advance queue after prepare timeout failed: {err:?}");
        }
    });
}

async fn notify_playback_issue(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    message: String,
) {
    let channel_id = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        state.player_channel_id.or(state.queue_channel_id)
    };

    if let Some(channel_id) = channel_id {
        if let Err(err) = channel_id.say(ctx, message).await {
            tracing::warn!("failed to send playback issue message: {err:?}");
        }
    }
}

fn spawn_idle_disconnect(ctx: serenity::Context, data: Data, guild_id: serenity::GuildId) {
    tokio::spawn(async move {
        let idle_timeout = data
            .db
            .idle_timeout_secs(guild_id)
            .map(Duration::from_secs)
            .unwrap_or_else(|_| Duration::from_secs(60));
        tokio::time::sleep(idle_timeout).await;

        let should_disconnect = {
            let state_lock = data.music.get(guild_id).await;
            let state = state_lock.lock().await;
            state.now_playing.is_none() && state.queue.is_empty() && state.current_handle.is_none()
        };

        if !should_disconnect {
            return;
        }

        let Some(manager) = songbird::get(&ctx).await else {
            tracing::warn!("songbird voice client unavailable during idle disconnect");
            return;
        };

        if manager.get(guild_id).is_some() {
            tracing::info!("disconnecting from voice after idle timeout");
            if let Err(err) = manager.remove(guild_id).await {
                tracing::warn!("idle voice disconnect failed: {err:?}");
            }
        }
    });
}

pub async fn seek(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    position: Duration,
) -> Result<(), Error> {
    let filtered = data.db.normalize_enabled(guild_id)?;
    if filtered {
        let (handle, track) = {
            let state_lock = data.music.get(guild_id).await;
            let mut state = state_lock.lock().await;
            let track = state
                .now_playing
                .clone()
                .ok_or("Tidak ada lagu yang sedang diputar.")?;
            let handle = state.current_handle.take();
            state.is_paused = false;
            (handle, track)
        };

        if let Some(handle) = handle {
            handle.stop().ok();
        }
        play_track_at(ctx, data, guild_id, track, Some(position)).await?;
    } else {
        let handle = {
            let state_lock = data.music.get(guild_id).await;
            let state = state_lock.lock().await;
            state
                .current_handle
                .clone()
                .ok_or("Tidak ada lagu yang sedang diputar.")?
        };
        handle.seek_async(position).await?;
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

    Ok(())
}

struct TrackPlayableNotifier {
    data: Data,
    guild_id: serenity::GuildId,
    track: Track,
    record_history: bool,
}

#[async_trait::async_trait]
impl EventHandler for TrackPlayableNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(tracks) = ctx {
            for (state, _) in *tracks {
                tracing::info!(
                    ready = ?state.ready,
                    playing = ?state.playing,
                    position = ?state.position,
                    play_time = ?state.play_time,
                    title = %self.track.title,
                    "songbird track playable"
                );
            }
        }

        if self.record_history {
            if let Err(err) = self.data.db.record_history(self.guild_id, &self.track) {
                tracing::warn!("failed to record track history: {err:?}");
            }
        }

        Some(Event::Cancel)
    }
}

struct TrackErrorNotifier {
    ctx: serenity::Context,
    data: Data,
    guild_id: serenity::GuildId,
}

#[async_trait::async_trait]
impl EventHandler for TrackErrorNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let mut errored_uuid = None;

        if let EventContext::Track(tracks) = ctx {
            for (state, handle) in *tracks {
                errored_uuid = Some(handle.uuid());
                tracing::warn!(
                    ready = ?state.ready,
                    playing = ?state.playing,
                    position = ?state.position,
                    play_time = ?state.play_time,
                    track_uuid = ?handle.uuid(),
                    "songbird track error; advancing queue"
                );
            }
        }

        let Some(errored_uuid) = errored_uuid else {
            return Some(Event::Cancel);
        };
        if !claim_current_handle(&self.data, self.guild_id, errored_uuid).await {
            tracing::debug!(?errored_uuid, "ignoring error from stale track handle");
            return Some(Event::Cancel);
        }

        notify_playback_issue(
            &self.ctx,
            &self.data,
            self.guild_id,
            "Current track failed to play, skipping to the next track.".to_string(),
        )
        .await;

        if let Err(err) = advance_queue(&self.ctx, &self.data, self.guild_id).await {
            tracing::warn!("advance queue after track error failed: {err:?}");
        }

        Some(Event::Cancel)
    }
}

struct TrackEndNotifier {
    ctx: serenity::Context,
    data: Data,
    guild_id: serenity::GuildId,
}

#[async_trait::async_trait]
impl EventHandler for TrackEndNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let mut ended_uuid = None;
        if let EventContext::Track(tracks) = ctx {
            for (_, handle) in *tracks {
                ended_uuid = Some(handle.uuid());
            }
        }
        let Some(ended_uuid) = ended_uuid else {
            return Some(Event::Cancel);
        };
        if !claim_current_handle(&self.data, self.guild_id, ended_uuid).await {
            tracing::debug!(?ended_uuid, "ignoring end from stale track handle");
            return Some(Event::Cancel);
        }

        if let Err(err) = advance_queue(&self.ctx, &self.data, self.guild_id).await {
            tracing::warn!("advance queue failed: {err:?}");
        }

        Some(Event::Cancel)
    }
}

async fn claim_current_handle(
    data: &Data,
    guild_id: serenity::GuildId,
    event_uuid: uuid::Uuid,
) -> bool {
    let state_lock = data.music.get(guild_id).await;
    let mut state = state_lock.lock().await;
    if state
        .current_handle
        .as_ref()
        .is_some_and(|handle| handle.uuid() == event_uuid)
    {
        state.current_handle = None;
        true
    } else {
        false
    }
}

/// Skip / lagu selesai -> tentukan lagu berikutnya.
pub async fn advance_queue(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let state_lock = data.music.get(guild_id).await;

    let (next_track, autoplay_requester, autoplay_exclude_url) = {
        let mut state = state_lock.lock().await;

        let finished = state.now_playing.take();
        let autoplay_requester = finished.as_ref().map(|track| track.requested_by);
        let autoplay_exclude_url = finished.as_ref().map(|track| track.url.clone());
        state.current_handle = None;
        state.skip_votes.clear();

        let next_track = match state.loop_mode {
            crate::music::state::LoopMode::One => {
                if let Some(track) = finished {
                    state.now_playing = Some(track.clone());
                    Some(track)
                } else {
                    None
                }
            }
            crate::music::state::LoopMode::Queue => {
                if let Some(track) = finished {
                    remember_previous(&mut state.previous_tracks, track.clone());
                    state.queue.push_back(track);
                }
                let next = state.queue.pop_front();
                state.now_playing = next.clone();
                next
            }
            crate::music::state::LoopMode::Off => {
                if let Some(track) = finished {
                    remember_previous(&mut state.previous_tracks, track);
                }
                let next = state.queue.pop_front();
                state.now_playing = next.clone();
                next
            }
        };

        (next_track, autoplay_requester, autoplay_exclude_url)
    };

    let next_track = if next_track.is_none() {
        autoplay_track(
            data,
            guild_id,
            autoplay_requester,
            autoplay_exclude_url.as_deref(),
        )
        .await?
    } else {
        next_track
    };

    let started_track = if let Some(track) = next_track {
        {
            let state_lock = data.music.get(guild_id).await;
            let mut state = state_lock.lock().await;
            state.now_playing = Some(track.clone());
        }

        play_track(ctx, data, guild_id, track).await?;
        true
    } else {
        false
    };

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();

    if !started_track {
        spawn_idle_disconnect(ctx.clone(), data.clone(), guild_id);
    }

    persist_queue(data, guild_id).await;

    Ok(())
}

async fn autoplay_track(
    data: &Data,
    guild_id: serenity::GuildId,
    requester: Option<serenity::UserId>,
    exclude_url: Option<&str>,
) -> Result<Option<Track>, Error> {
    if !data.db.autoplay_enabled(guild_id)? {
        return Ok(None);
    }

    let Some(requester) = requester else {
        return Ok(None);
    };

    let track = data
        .db
        .random_history_track(guild_id, requester, exclude_url)?;
    if let Some(track) = &track {
        tracing::info!(title = %track.title, "autoplay selected history track");
    }

    Ok(track)
}

pub async fn stop(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue.clear();
        state.now_playing = None;
        let current_handle = state.current_handle.take();
        state.is_paused = false;
        state.skip_votes.clear();

        if let Some(handle) = current_handle {
            handle.stop().ok();
        }
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();

    spawn_idle_disconnect(ctx.clone(), data.clone(), guild_id);
    persist_queue(data, guild_id).await;

    Ok(())
}

pub async fn skip(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let (current_handle, next_track) = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let current_handle = state.current_handle.take();
        state.is_paused = false;
        state.skip_votes.clear();

        let finished = state.now_playing.take();

        if state.loop_mode == crate::music::state::LoopMode::Queue {
            if let Some(track) = finished {
                remember_previous(&mut state.previous_tracks, track.clone());
                state.queue.push_back(track);
            }
        } else if let Some(track) = finished {
            remember_previous(&mut state.previous_tracks, track);
        }

        let next_track = state.queue.pop_front();
        state.now_playing = next_track.clone();

        (current_handle, next_track)
    };

    if let Some(handle) = current_handle {
        handle.stop().ok();
    }

    if let Some(track) = next_track {
        play_track(ctx, data, guild_id, track).await?;
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();
    persist_queue(data, guild_id).await;

    Ok(())
}

fn remember_previous(previous_tracks: &mut std::collections::VecDeque<Track>, track: Track) {
    previous_tracks.push_back(track);
    while previous_tracks.len() > 20 {
        previous_tracks.pop_front();
    }
}

pub async fn pause_resume(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let should_pause = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.is_paused = !state.is_paused;
        state.is_paused
    };

    let current_handle = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        state.current_handle.clone()
    };

    if let Some(handle) = current_handle {
        if should_pause {
            handle.pause()?;
        } else {
            handle.play()?;
        }
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

    Ok(())
}

pub async fn leave(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    stop(ctx, data, guild_id).await.ok();

    let manager = songbird::get(ctx)
        .await
        .ok_or("Songbird voice client belum tersedia.")?
        .clone();

    let _ = manager.remove(guild_id).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_permission_diagnostics_are_specific() {
        assert_eq!(
            missing_voice_permissions(serenity::Permissions::empty()),
            vec!["Connect", "Speak"]
        );
        assert_eq!(
            missing_voice_permissions(serenity::Permissions::CONNECT),
            vec!["Speak"]
        );
        assert!(missing_voice_permissions(
            serenity::Permissions::CONNECT | serenity::Permissions::SPEAK
        )
        .is_empty());
    }
}
