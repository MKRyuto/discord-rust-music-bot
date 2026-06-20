use poise::serenity_prelude as serenity;
use serenity::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateMessage, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption, EditMessage,
};

use crate::{music::state::LoopMode, Data, Error};

pub const BTN_PAUSE_RESUME: &str = "music:pause_resume";
pub const BTN_SKIP: &str = "music:skip";
pub const BTN_PREVIOUS: &str = "music:previous";
pub const BTN_REPLAY: &str = "music:replay";
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
pub const BTN_VOLUME_50: &str = "music:volume_50";
pub const BTN_VOLUME_100: &str = "music:volume_100";
pub const BTN_VOLUME_150: &str = "music:volume_150";
pub const SELECT_LOOP_MODE: &str = "music:loop_select";
pub const SELECT_PLAYLIST_LOAD: &str = "music:playlist_load_select";

pub async fn build_player_embed(data: &Data, guild_id: serenity::GuildId) -> CreateEmbed {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    let autoplay = data.db.autoplay_enabled(guild_id).unwrap_or(false);
    let normalize = data.db.normalize_enabled(guild_id).unwrap_or(false);
    let normalize_cap = data.db.normalize_cap_percent(guild_id).unwrap_or(85);

    let mut embed = CreateEmbed::new();

    if let Some(track) = &state.now_playing {
        let status = if state.is_paused { "Paused" } else { "Playing" };

        let normalize_label = if normalize {
            format!("On (cap {normalize_cap}%)")
        } else {
            "Off".to_string()
        };

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
    has_track: bool,
    has_previous: bool,
    has_queue: bool,
    loop_mode: LoopMode,
    normalize_enabled: bool,
    autoplay_enabled: bool,
    playlists: Vec<crate::storage::PlaylistSummary>,
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
                .style(ButtonStyle::Primary)
                .disabled(!has_track),
            CreateButton::new(BTN_SKIP)
                .label("Skip")
                .style(ButtonStyle::Secondary)
                .disabled(!has_track),
            CreateButton::new(BTN_PREVIOUS)
                .label("Previous")
                .style(ButtonStyle::Secondary)
                .disabled(!has_previous),
            CreateButton::new(BTN_REPLAY)
                .label("Replay")
                .style(ButtonStyle::Secondary)
                .disabled(!has_track),
            CreateButton::new(BTN_STOP)
                .label("Stop")
                .style(ButtonStyle::Danger)
                .disabled(!has_track),
        ]),
        CreateActionRow::Buttons(vec![
            CreateButton::new(BTN_QUEUE)
                .label("Queue")
                .style(ButtonStyle::Secondary)
                .disabled(!has_track && !has_queue),
            CreateButton::new(BTN_VOTE_SKIP)
                .label("Vote Skip")
                .style(ButtonStyle::Secondary)
                .disabled(!has_track),
            CreateButton::new(BTN_REFRESH)
                .label("Refresh")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_VOLUME_DOWN)
                .label("Vol -")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_VOLUME_UP)
                .label("Vol +")
                .style(ButtonStyle::Secondary),
        ]),
        CreateActionRow::Buttons(vec![
            CreateButton::new(BTN_SHUFFLE)
                .label("Shuffle")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_NORMALIZE)
                .label(normalize_label)
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_AUTOPLAY)
                .label(autoplay_label)
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_VOLUME_50)
                .label("50%")
                .style(ButtonStyle::Secondary),
            CreateButton::new(BTN_VOLUME_100)
                .label("100%")
                .style(ButtonStyle::Secondary),
        ]),
        CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                SELECT_LOOP_MODE,
                CreateSelectMenuKind::String {
                    options: loop_options(loop_mode),
                },
            )
            .placeholder("Loop mode")
            .min_values(1)
            .max_values(1),
        ),
    ]
    .into_iter()
    .chain(playlist_select_row(playlists))
    .collect()
}

pub async fn build_player_components(
    data: &Data,
    guild_id: serenity::GuildId,
) -> Vec<CreateActionRow> {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    let normalize_enabled = data.db.normalize_enabled(guild_id).unwrap_or(false);
    let autoplay_enabled = data.db.autoplay_enabled(guild_id).unwrap_or(false);
    let playlists = data.db.list_playlists(guild_id).unwrap_or_default();
    build_player_buttons(
        state.is_paused,
        state.now_playing.is_some(),
        !state.previous_tracks.is_empty(),
        !state.queue.is_empty(),
        state.loop_mode,
        normalize_enabled,
        autoplay_enabled,
        playlists,
    )
}

fn loop_options(current: LoopMode) -> Vec<CreateSelectMenuOption> {
    [
        (LoopMode::Off, "Off", "No looping"),
        (LoopMode::One, "One", "Repeat current track"),
        (LoopMode::Queue, "Queue", "Repeat the queue"),
    ]
    .into_iter()
    .map(|(mode, label, description)| {
        CreateSelectMenuOption::new(label, label.to_lowercase())
            .description(description)
            .default_selection(mode == current)
    })
    .collect()
}

fn playlist_select_row(playlists: Vec<crate::storage::PlaylistSummary>) -> Option<CreateActionRow> {
    let options = playlists
        .into_iter()
        .take(25)
        .map(|playlist| {
            CreateSelectMenuOption::new(
                truncate_option(&playlist.name, 90),
                truncate_option(&playlist.name, 100),
            )
            .description(format!("{} track(s)", playlist.track_count))
        })
        .collect::<Vec<_>>();

    if options.is_empty() {
        return None;
    }

    Some(CreateActionRow::SelectMenu(
        CreateSelectMenu::new(
            SELECT_PLAYLIST_LOAD,
            CreateSelectMenuKind::String { options },
        )
        .placeholder("Load playlist")
        .min_values(1)
        .max_values(1),
    ))
}

fn truncate_option(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
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
