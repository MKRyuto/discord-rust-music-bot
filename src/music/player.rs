use poise::serenity_prelude as serenity;
use songbird::input::YoutubeDl;
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
        let mut state = state_lock.lock().await;
        state.current_handle = Some(track_handle.clone());
    }

    track_handle.add_event(
        Event::Track(TrackEvent::Playable),
        TrackStateLogger {
            event_name: "playable",
        },
    )?;

    track_handle.add_event(
        Event::Track(TrackEvent::Error),
        TrackStateLogger {
            event_name: "error",
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

    Ok(())
}

struct TrackStateLogger {
    event_name: &'static str,
}

#[async_trait::async_trait]
impl EventHandler for TrackStateLogger {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(tracks) = ctx {
            for (state, _) in *tracks {
                tracing::warn!(
                    event = self.event_name,
                    ready = ?state.ready,
                    playing = ?state.playing,
                    position = ?state.position,
                    play_time = ?state.play_time,
                    "songbird track event"
                );
            }
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

    let next_track = {
        let mut state = state_lock.lock().await;

        let finished = state.now_playing.take();
        state.current_handle = None;

        match state.loop_mode {
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
        }
    };

    if let Some(track) = next_track {
        play_track(ctx, data, guild_id, track).await?;
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();

    Ok(())
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
