use poise::serenity_prelude as serenity;
use serenity::{
    ComponentInteractionDataKind, CreateInteractionResponse, CreateInteractionResponseMessage,
    FullEvent, Interaction,
};

use crate::{
    music::player,
    music::state::LoopMode,
    permissions,
    ui::{player_panel, queue_panel},
    Data, Error,
};

pub async fn handle_event(
    ctx: &serenity::Context,
    event: &FullEvent,
    data: &Data,
) -> Result<(), Error> {
    let FullEvent::InteractionCreate { interaction } = event else {
        return Ok(());
    };

    let Interaction::Component(component) = interaction else {
        return Ok(());
    };

    let custom_id = component.data.custom_id.as_str();

    if !custom_id.starts_with("music:") {
        return Ok(());
    }

    let Some(guild_id) = component.guild_id else {
        return Ok(());
    };

    if requires_music_control(custom_id)
        && !component_can_control(ctx, data, component, guild_id).await?
    {
        component
            .create_response(
                ctx,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(
                            "Kontrol ini cuma bisa dipakai admin server atau role DJ yang sudah diset.",
                        )
                        .ephemeral(true),
                ),
            )
            .await?;
        return Ok(());
    }

    match custom_id {
        player_panel::BTN_PAUSE_RESUME => {
            component.defer(ctx).await?;
            player::pause_resume(ctx, data, guild_id).await?;
        }
        player_panel::BTN_SKIP => {
            component.defer(ctx).await?;
            player::skip(ctx, data, guild_id).await?;
        }
        player_panel::BTN_PREVIOUS => {
            respond_ephemeral(
                ctx,
                component,
                match player::previous(ctx, data, guild_id).await {
                    Ok(()) => "Playing previous track.".to_string(),
                    Err(err) => err.to_string(),
                },
            )
            .await?;
        }
        player_panel::BTN_REPLAY => {
            respond_ephemeral(
                ctx,
                component,
                match player::replay(ctx, data, guild_id).await {
                    Ok(()) => "Replaying current track.".to_string(),
                    Err(err) => err.to_string(),
                },
            )
            .await?;
        }
        player_panel::BTN_VOTE_SKIP => {
            let message = match player::vote_skip(ctx, data, guild_id, component.user.id).await {
                Ok((votes, needed, skipped)) => {
                    if skipped {
                        format!("Vote skip lolos `{votes}/{needed}`. Skipping.")
                    } else {
                        format!("Vote skip: `{votes}/{needed}` vote(s).")
                    }
                }
                Err(err) => err.to_string(),
            };
            respond_ephemeral(ctx, component, message).await?;
        }
        player_panel::BTN_STOP => {
            component.defer(ctx).await?;
            player::stop(ctx, data, guild_id).await?;
        }
        player_panel::BTN_LOOP => {
            {
                let state_lock = data.music.get(guild_id).await;
                let mut state = state_lock.lock().await;
                state.loop_mode = state.loop_mode.next();
            }

            update_component_to_player(ctx, data, component, guild_id).await?;
        }
        player_panel::SELECT_LOOP_MODE => {
            let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
                return Ok(());
            };

            let Some(mode) = values.first().and_then(|value| loop_mode_from_value(value)) else {
                return Ok(());
            };

            {
                let state_lock = data.music.get(guild_id).await;
                let mut state = state_lock.lock().await;
                state.loop_mode = mode;
            }

            update_component_to_player(ctx, data, component, guild_id).await?;
        }
        player_panel::BTN_VOLUME_DOWN => {
            component.defer(ctx).await?;
            player::adjust_volume(ctx, data, guild_id, -10).await?;
        }
        player_panel::BTN_VOLUME_UP => {
            component.defer(ctx).await?;
            player::adjust_volume(ctx, data, guild_id, 10).await?;
        }
        player_panel::BTN_VOLUME_50 => {
            component.defer(ctx).await?;
            player::set_volume(ctx, data, guild_id, 50).await?;
        }
        player_panel::BTN_VOLUME_100 => {
            component.defer(ctx).await?;
            player::set_volume(ctx, data, guild_id, 100).await?;
        }
        player_panel::BTN_VOLUME_150 => {
            component.defer(ctx).await?;
            player::set_volume(ctx, data, guild_id, 150).await?;
        }
        player_panel::BTN_SHUFFLE => {
            let total = player::shuffle_queue(data, guild_id).await;
            update_component_to_player(ctx, data, component, guild_id).await?;

            if total > 0 {
                player_panel::update_player_message(ctx, data, guild_id)
                    .await
                    .ok();
            }
        }
        player_panel::BTN_NORMALIZE => {
            let enabled = !data.db.normalize_enabled(guild_id)?;
            data.db.set_normalize_enabled(guild_id, enabled)?;
            update_component_to_player(ctx, data, component, guild_id).await?;
            player::set_volume(ctx, data, guild_id, current_volume(data, guild_id).await).await?;
        }
        player_panel::BTN_AUTOPLAY => {
            let enabled = !data.db.autoplay_enabled(guild_id)?;
            data.db.set_autoplay_enabled(guild_id, enabled)?;
            update_component_to_player(ctx, data, component, guild_id).await?;
        }
        player_panel::BTN_PLAYLISTS => {
            show_playlists(ctx, data, component, guild_id).await?;
        }
        player_panel::SELECT_PLAYLIST_LOAD => {
            let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
                return Ok(());
            };

            let Some(name) = values.first() else {
                return Ok(());
            };

            let message = load_playlist_from_select(ctx, data, guild_id, component, name).await;
            respond_ephemeral(ctx, component, message).await?;
        }
        player_panel::BTN_REFRESH | queue_panel::BTN_PLAYER => {
            update_component_to_player(ctx, data, component, guild_id).await?;
        }
        player_panel::BTN_QUEUE => {
            update_component_to_queue(ctx, data, component, guild_id).await?;
        }
        queue_panel::BTN_PREV => {
            {
                let state_lock = data.music.get(guild_id).await;
                let mut state = state_lock.lock().await;
                state.queue_page = state.queue_page.saturating_sub(1);
            }

            update_component_to_queue(ctx, data, component, guild_id).await?;
        }
        queue_panel::BTN_NEXT => {
            {
                let state_lock = data.music.get(guild_id).await;
                let mut state = state_lock.lock().await;
                let max_page = state.queue.len().div_ceil(10).saturating_sub(1);
                state.queue_page = (state.queue_page + 1).min(max_page);
            }

            update_component_to_queue(ctx, data, component, guild_id).await?;
        }
        queue_panel::BTN_CLEAR => {
            component
                .create_response(
                    ctx,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Clear all queued tracks?")
                            .components(queue_panel::clear_confirm_buttons())
                            .ephemeral(true),
                    ),
                )
                .await?;
        }
        queue_panel::BTN_CLEAR_CONFIRM => {
            clear_queue(ctx, data, guild_id).await;
            component
                .create_response(
                    ctx,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("Queue cleared.")
                            .components(Vec::new()),
                    ),
                )
                .await?;
        }
        queue_panel::BTN_CLEAR_CANCEL => {
            component
                .create_response(
                    ctx,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("Clear cancelled.")
                            .components(Vec::new()),
                    ),
                )
                .await?;
        }
        queue_panel::SELECT_PAGE => {
            let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
                return Ok(());
            };

            let Some(page) = values.first().and_then(|value| value.parse::<usize>().ok()) else {
                return Ok(());
            };

            player::set_queue_page(data, guild_id, page).await;
            update_component_to_queue(ctx, data, component, guild_id).await?;
        }
        queue_panel::SELECT_REMOVE => {
            let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
                return Ok(());
            };

            let Some(position) = values.first().and_then(|value| value.parse::<usize>().ok())
            else {
                return Ok(());
            };

            let removed = player::remove_queued_track(data, guild_id, position).await;
            update_component_to_queue(ctx, data, component, guild_id).await?;
            player_panel::update_player_message(ctx, data, guild_id)
                .await
                .ok();

            if let Some(track) = removed {
                tracing::info!(title = %track.title, position, "removed queued track from select menu");
            }
        }
        queue_panel::SELECT_REMOVE_RANGE => {
            let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
                return Ok(());
            };

            let Some((start, end)) = values.first().and_then(|value| parse_range(value)) else {
                return Ok(());
            };

            player::remove_queued_track_range(data, guild_id, start, end).await;
            update_component_to_queue(ctx, data, component, guild_id).await?;
            player_panel::update_player_message(ctx, data, guild_id)
                .await
                .ok();
        }
        _ => {}
    }

    Ok(())
}

fn requires_music_control(custom_id: &str) -> bool {
    matches!(
        custom_id,
        player_panel::BTN_PAUSE_RESUME
            | player_panel::BTN_SKIP
            | player_panel::BTN_PREVIOUS
            | player_panel::BTN_REPLAY
            | player_panel::BTN_STOP
            | player_panel::BTN_LOOP
            | player_panel::SELECT_LOOP_MODE
            | player_panel::SELECT_PLAYLIST_LOAD
            | player_panel::BTN_VOLUME_DOWN
            | player_panel::BTN_VOLUME_UP
            | player_panel::BTN_VOLUME_50
            | player_panel::BTN_VOLUME_100
            | player_panel::BTN_VOLUME_150
            | player_panel::BTN_SHUFFLE
            | player_panel::BTN_NORMALIZE
            | player_panel::BTN_AUTOPLAY
            | queue_panel::BTN_CLEAR
            | queue_panel::BTN_CLEAR_CONFIRM
            | queue_panel::SELECT_PAGE
            | queue_panel::SELECT_REMOVE_RANGE
            | queue_panel::SELECT_REMOVE
    )
}

async fn respond_ephemeral(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    message: String,
) -> Result<(), Error> {
    component
        .create_response(
            ctx,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(message)
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}

fn loop_mode_from_value(value: &str) -> Option<LoopMode> {
    match value {
        "off" => Some(LoopMode::Off),
        "one" => Some(LoopMode::One),
        "queue" => Some(LoopMode::Queue),
        _ => None,
    }
}

fn parse_range(value: &str) -> Option<(usize, usize)> {
    let (start, end) = value.split_once(':')?;
    Some((start.parse().ok()?, end.parse().ok()?))
}

async fn clear_queue(ctx: &serenity::Context, data: &Data, guild_id: serenity::GuildId) {
    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue.clear();
        state.queue_page = 0;
    }

    player::persist_queue(data, guild_id).await;
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();
    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
}

async fn load_playlist_from_select(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    component: &serenity::ComponentInteraction,
    name: &str,
) -> String {
    let tracks = match data.db.load_playlist(guild_id, name, component.user.id) {
        Ok(tracks) if !tracks.is_empty() => tracks,
        Ok(_) => return format!("Playlist `{name}` kosong atau tidak ditemukan."),
        Err(err) => return format!("Gagal load playlist: {err}"),
    };

    if let Err(err) = player::join_user_channel(ctx, guild_id, component.user.id).await {
        return format!("Gagal join voice channel: {err}");
    }

    let count = tracks.len();
    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue.extend(tracks);
        state.player_channel_id = Some(component.channel_id);
    }

    player::persist_queue(data, guild_id).await;
    if let Err(err) = player::start_if_idle(ctx, data, guild_id).await {
        return format!("Gagal mulai playlist: {err}");
    }

    player_panel::update_player_message(ctx, data, guild_id)
        .await
        .ok();
    queue_panel::update_queue_message(ctx, data, guild_id)
        .await
        .ok();

    format!("Loaded playlist `{name}` with `{count}` track(s).")
}

async fn current_volume(data: &Data, guild_id: serenity::GuildId) -> u8 {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    state.volume_percent
}

async fn component_can_control(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<bool, Error> {
    let allowed_channels = data.db.allowed_channels(guild_id)?;
    if !allowed_channels.is_empty() && !allowed_channels.contains(&component.channel_id) {
        return Ok(false);
    }

    let Some(member) = component.member.as_ref() else {
        return Ok(false);
    };

    permissions::can_control_music(ctx, data, guild_id, member).await
}

async fn show_playlists(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let playlists = data.db.list_playlists(guild_id)?;
    let content = if playlists.is_empty() {
        "Belum ada saved playlist. Pakai `/playlist save name:<nama>` buat simpan queue sekarang."
            .to_string()
    } else {
        let list = playlists
            .iter()
            .take(10)
            .map(|playlist| {
                format!(
                    "- `{}` - `{}` track(s)",
                    playlist.name, playlist.track_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Saved playlists:\n{list}\n\nLoad pakai `/playlist load name:<nama>`. Save queue sekarang pakai `/playlist save name:<nama>`."
        )
    };

    component
        .create_response(
            ctx,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}

async fn update_component_to_player(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let embed = player_panel::build_player_embed(data, guild_id).await;
    let components = player_panel::build_player_components(data, guild_id).await;

    component
        .create_response(
            ctx,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(components),
            ),
        )
        .await?;

    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.player_message_id = Some(component.message.id);
        state.player_channel_id = Some(component.channel_id);
    }

    Ok(())
}

async fn update_component_to_queue(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let embed = queue_panel::build_queue_embed(data, guild_id).await;

    component
        .create_response(
            ctx,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(queue_panel::build_queue_buttons(data, guild_id).await),
            ),
        )
        .await?;

    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue_message_id = Some(component.message.id);
        state.queue_channel_id = Some(component.channel_id);
    }

    Ok(())
}
