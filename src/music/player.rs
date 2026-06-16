use std::time::{Duration, SystemTime, UNIX_EPOCH};

use poise::serenity_prelude as serenity;
use songbird::input::YoutubeDl;
use songbird::tracks::{ReadyState, TrackHandle};
use songbird::{Event, EventContext, EventHandler, TrackEvent};

use crate::music::track::Track;
use crate::ui::player_panel;
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
    data.db.set_guild_volume(guild_id, volume_percent)?;

    let current_handle = {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.volume_percent = volume_percent;
        state.volume_loaded = true;
        state.current_handle.clone()
    };

    if let Some(handle) = current_handle {
        handle.set_volume(volume_percent as f32 / 100.0)?;
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

    Ok(())
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

    let manager = songbird::get(ctx)
        .await
        .ok_or("Songbird voice client belum terpasang.")?
        .clone();

    let _handler_lock = manager.join(guild_id, channel_id).await?;

    Ok(channel_id)
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
        track
    };

    play_track(ctx, data, guild_id, next_track).await?;
    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

    Ok(())
}

/// Play track sekarang.
pub async fn play_track(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    track: Track,
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

    let input = youtube_dl_for(data, track.url.clone());
    let track_handle = handler.play_input(input.into());

    {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        track_handle.set_volume(state.volume_percent as f32 / 100.0)?;
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

        {
            let state_lock = data.music.get(guild_id).await;
            let mut music_state = state_lock.lock().await;
            if music_state
                .current_handle
                .as_ref()
                .is_some_and(|handle| handle.uuid() == track_handle.uuid())
            {
                music_state.suppress_next_end = true;
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

struct TrackPlayableNotifier {
    data: Data,
    guild_id: serenity::GuildId,
    track: Track,
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

        if let Err(err) = self.data.db.record_history(self.guild_id, &self.track) {
            tracing::warn!("failed to record track history: {err:?}");
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

        if let Some(errored_uuid) = errored_uuid {
            let state_lock = self.data.music.get(self.guild_id).await;
            let mut state = state_lock.lock().await;
            if state
                .current_handle
                .as_ref()
                .is_some_and(|handle| handle.uuid() == errored_uuid)
            {
                state.suppress_next_end = true;
            }
        }

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
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let should_suppress = {
            let state_lock = self.data.music.get(self.guild_id).await;
            let mut state = state_lock.lock().await;
            if state.suppress_next_end {
                state.suppress_next_end = false;
                true
            } else {
                false
            }
        };

        if should_suppress {
            player_panel::update_player_message(&self.ctx, &self.data, self.guild_id)
                .await
                .ok();
            return Some(Event::Cancel);
        }

        if let Err(err) = advance_queue(&self.ctx, &self.data, self.guild_id).await {
            tracing::warn!("advance queue failed: {err:?}");
        }

        Some(Event::Cancel)
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
                    state.queue.push_back(track);
                }
                let next = state.queue.pop_front();
                state.now_playing = next.clone();
                next
            }
            crate::music::state::LoopMode::Off => {
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

    if let Some(track) = next_track {
        {
            let state_lock = data.music.get(guild_id).await;
            let mut state = state_lock.lock().await;
            state.now_playing = Some(track.clone());
        }

        play_track(ctx, data, guild_id, track).await?;
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

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
        state.suppress_next_end = current_handle.is_some();
        state.is_paused = false;

        if let Some(handle) = current_handle {
            handle.stop().ok();
        }
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

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
        state.suppress_next_end = current_handle.is_some();
        state.is_paused = false;

        let finished = state.now_playing.take();

        if state.loop_mode == crate::music::state::LoopMode::Queue {
            if let Some(track) = finished {
                state.queue.push_back(track);
            }
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

    Ok(())
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
