use poise::serenity_prelude as serenity;
use serenity::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateMessage, EditMessage,
};

use crate::{Data, Error};

pub const BTN_PAUSE_RESUME: &str = "music:pause_resume";
pub const BTN_SKIP: &str = "music:skip";
pub const BTN_STOP: &str = "music:stop";
pub const BTN_QUEUE: &str = "music:queue";
pub const BTN_LOOP: &str = "music:loop";
pub const BTN_REFRESH: &str = "music:refresh_player";
pub const BTN_VOLUME_DOWN: &str = "music:volume_down";
pub const BTN_VOLUME_UP: &str = "music:volume_up";
pub const BTN_SHUFFLE: &str = "music:shuffle";
pub const BTN_PLAYLISTS: &str = "music:playlists";
pub const BTN_VOTE_SKIP: &str = "music:vote_skip";
pub const BTN_NORMALIZE: &str = "music:normalize";
pub const BTN_AUTOPLAY: &str = "music:autoplay";

pub async fn build_player_embed(data: &Data, guild_id: serenity::GuildId) -> CreateEmbed {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    let autoplay = data.db.autoplay_enabled(guild_id).unwrap_or(false);
    let normalize = data.db.normalize_enabled(guild_id).unwrap_or(false);

    let mut embed = CreateEmbed::new();

    if let Some(track) = &state.now_playing {
        let status = if state.is_paused { "Paused" } else { "Playing" };

        let normalize_label = if normalize { "On (cap 85%)" } else { "Off" };

        embed = embed.title("Music Player").description(format!(
            "{status}\n\n**{}**\nDuration: `{}`\nRequested by: <@{}>\n\nQueue: `{}` track(s)\nVote skip: `{}` vote(s)\nLoop: `{}`\nVolume: `{}%`\nAutoplay: `{}`\nNormalize: `{}`",
            track.title,
            track.duration_label(),
            track.requested_by.get(),
            state.queue.len(),
            state.skip_votes.len(),
            state.loop_mode.label(),
            state.volume_percent,
            if autoplay { "On" } else { "Off" },
            normalize_label,
        ));

        if let Some(thumbnail) = &track.thumbnail {
            embed = embed.thumbnail(thumbnail);
        }
    } else {
        embed = embed
            .title("Music Player")
            .description("Nothing is playing.\n\nUse `/play <url or song title>`.");
    }

    embed
}

pub fn build_player_buttons(
    is_paused: bool,
    loop_label: &str,
    normalize_enabled: bool,
    autoplay_enabled: bool,
) -> Vec<CreateActionRow> {
    let pause_label = if is_paused { "Resume" } else { "Pause" };
    let normalize_label = if normalize_enabled {
        "Normalize: On"
    } else {
        "Normalize: Off"
    };
    let autoplay_label = if autoplay_enabled {
        "Autoplay: On"
    } else {
        "Autoplay: Off"
    };

    vec![
        CreateActionRow::Buttons(vec![
            CreateButton::new(BTN_PAUSE_RESUME)
                .label(pause_label)
                .style(ButtonStyle::Primary),
            CreateButton::new(BTN_SKIP)
                .label("Skip")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_VOTE_SKIP)
                .label("Vote Skip")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_STOP)
                .label("Stop")
                .style(ButtonStyle::Danger),
        ]),
        CreateActionRow::Buttons(vec![
            CreateButton::new(BTN_QUEUE)
                .label("Queue")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_LOOP)
                .label(format!("Loop: {loop_label}"))
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_REFRESH)
                .label("Refresh")
                .style(ButtonStyle::Secondary),
        ]),
        CreateActionRow::Buttons(vec![
            CreateButton::new(BTN_VOLUME_DOWN)
                .label("Vol -")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_VOLUME_UP)
                .label("Vol +")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_SHUFFLE)
                .label("Shuffle")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_NORMALIZE)
                .label(normalize_label)
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_PLAYLISTS)
                .label("Playlists")
                .style(ButtonStyle::Secondary),
        ]),
        CreateActionRow::Buttons(vec![CreateButton::new(BTN_AUTOPLAY)
            .label(autoplay_label)
            .style(ButtonStyle::Secondary)]),
    ]
}

pub async fn build_player_components(
    data: &Data,
    guild_id: serenity::GuildId,
) -> Vec<CreateActionRow> {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    let normalize_enabled = data.db.normalize_enabled(guild_id).unwrap_or(false);
    let autoplay_enabled = data.db.autoplay_enabled(guild_id).unwrap_or(false);
    build_player_buttons(
        state.is_paused,
        state.loop_mode.label(),
        normalize_enabled,
        autoplay_enabled,
    )
}

pub async fn send_or_update_player_panel(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) -> Result<(), Error> {
    let embed = build_player_embed(data, guild_id).await;
    let components = build_player_components(data, guild_id).await;

    let maybe_msg_id = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        state.player_message_id
    };

    if let Some(message_id) = maybe_msg_id {
        let edit = EditMessage::new().embed(embed).components(components);

        match channel_id.edit_message(ctx, message_id, edit).await {
            Ok(_) => return Ok(()),
            Err(err) => tracing::warn!("failed edit player panel, send new one: {err:?}"),
        }
    }

    let msg = channel_id
        .send_message(
            ctx,
            CreateMessage::new()
                .embed(build_player_embed(data, guild_id).await)
                .components(build_player_components(data, guild_id).await),
        )
        .await?;

    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.player_message_id = Some(msg.id);
        state.player_channel_id = Some(channel_id);
    }

    Ok(())
}

pub async fn update_player_message(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let channel_id = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        state.player_channel_id
    };

    if let Some(channel_id) = channel_id {
        send_or_update_player_panel(ctx, data, guild_id, channel_id).await?;
    }

    Ok(())
}
