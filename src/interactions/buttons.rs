use poise::serenity_prelude as serenity;
use serenity::{
    ComponentInteractionDataKind, CreateInteractionResponse, CreateInteractionResponseMessage,
    FullEvent, Interaction,
};

use crate::{
    music::player,
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

    match custom_id {
        player_panel::BTN_PAUSE_RESUME => {
            component.defer(ctx).await?;
            player::pause_resume(ctx, data, guild_id).await?;
        }
        player_panel::BTN_SKIP => {
            component.defer(ctx).await?;
            player::skip(ctx, data, guild_id).await?;
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
        player_panel::BTN_VOLUME_DOWN => {
            component.defer(ctx).await?;
            player::adjust_volume(ctx, data, guild_id, -10).await?;
        }
        player_panel::BTN_VOLUME_UP => {
            component.defer(ctx).await?;
            player::adjust_volume(ctx, data, guild_id, 10).await?;
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
        player_panel::BTN_PLAYLISTS => {
            show_playlists(ctx, data, component, guild_id).await?;
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
            {
                let state_lock = data.music.get(guild_id).await;
                let mut state = state_lock.lock().await;
                state.queue.clear();
                state.queue_page = 0;
            }

            player::persist_queue(data, guild_id).await;
            update_component_to_queue(ctx, data, component, guild_id).await?;
            player_panel::update_player_message(ctx, data, guild_id)
                .await
                .ok();
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
                component
                    .channel_id
                    .say(ctx, format!("Removed `{position}.` **{}**", track.title))
                    .await
                    .ok();
            }
        }
        _ => {}
    }

    Ok(())
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
