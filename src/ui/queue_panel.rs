use poise::serenity_prelude as serenity;
use serenity::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateMessage, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption, EditMessage,
};

use crate::{Data, Error};

pub const BTN_PREV: &str = "music:queue_prev";
pub const BTN_NEXT: &str = "music:queue_next";
pub const BTN_CLEAR: &str = "music:queue_clear";
pub const BTN_CLEAR_CONFIRM: &str = "music:queue_clear_confirm";
pub const BTN_CLEAR_CANCEL: &str = "music:queue_clear_cancel";
pub const BTN_PLAYER: &str = "music:player";
pub const SELECT_REMOVE: &str = "music:queue_remove_select";
pub const SELECT_PAGE: &str = "music:queue_page_select";
pub const SELECT_REMOVE_RANGE: &str = "music:queue_remove_range_select";

const PAGE_SIZE: usize = 10;

pub async fn build_queue_embed(data: &Data, guild_id: serenity::GuildId) -> CreateEmbed {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;

    let total = state.queue.len();
    let total_pages = total.div_ceil(PAGE_SIZE).max(1);
    let page = state.queue_page.min(total_pages - 1);
    let start = page * PAGE_SIZE;

    let mut desc = String::new();

    desc.push_str("**Now Playing:**\n");
    if let Some(track) = &state.now_playing {
        desc.push_str(&format!(
            "> **{}**\n`{}` - requested by <@{}>\n\n",
            track.title,
            track.duration_label(),
            track.requested_by.get(),
        ));
    } else {
        desc.push_str("_Tidak ada lagu berjalan._\n\n");
    }

    desc.push_str("**Up Next:**\n");

    if total == 0 {
        desc.push_str("_Queue kosong._\n");
    } else {
        for (idx, track) in state.queue.iter().skip(start).take(PAGE_SIZE).enumerate() {
            let number = start + idx + 1;
            desc.push_str(&format!(
                "`{number}.` **{}**\n`{}` - requested by <@{}>\n",
                track.title,
                track.duration_label(),
                track.requested_by.get(),
            ));
        }
    }

    desc.push_str(&format!(
        "\nPage `{}/{}` - Total `{}` lagu",
        page + 1,
        total_pages,
        total
    ));

    CreateEmbed::new().title("Queue").description(desc)
}

pub async fn build_queue_buttons(data: &Data, guild_id: serenity::GuildId) -> Vec<CreateActionRow> {
    let state_lock = data.music.get(guild_id).await;
    let state = state_lock.lock().await;
    let total_pages = state.queue.len().div_ceil(PAGE_SIZE).max(1);
    let page = state.queue_page.min(total_pages - 1);

    let mut rows = vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BTN_PREV)
            .label("Prev Page")
            .style(ButtonStyle::Secondary)
            .disabled(page == 0),
        CreateButton::new(BTN_NEXT)
            .label("Next Page")
            .style(ButtonStyle::Secondary)
            .disabled(page + 1 >= total_pages),
        CreateButton::new(BTN_CLEAR)
            .label("Clear")
            .style(ButtonStyle::Danger)
            .disabled(state.queue.is_empty()),
        CreateButton::new(BTN_PLAYER)
            .label("Player")
            .style(ButtonStyle::Primary),
    ])];

    if total_pages > 1 {
        let page_options = (0..total_pages.min(25))
            .map(|idx| {
                CreateSelectMenuOption::new(format!("Page {}", idx + 1), (idx + 1).to_string())
                    .description(format!("Jump to queue page {}", idx + 1))
                    .default_selection(idx == page)
            })
            .collect::<Vec<_>>();

        rows.push(CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                SELECT_PAGE,
                CreateSelectMenuKind::String {
                    options: page_options,
                },
            )
            .placeholder("Jump to page")
            .min_values(1)
            .max_values(1),
        ));
    }

    let start = page * PAGE_SIZE;
    let options = state
        .queue
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .enumerate()
        .map(|(idx, track)| {
            let position = start + idx + 1;
            CreateSelectMenuOption::new(
                format!("{position}. {}", truncate_option(&track.title, 86)),
                position.to_string(),
            )
            .description(format!(
                "{} - requested by {}",
                track.duration_label(),
                track.requested_by
            ))
        })
        .collect::<Vec<_>>();

    if !options.is_empty() {
        rows.push(CreateActionRow::SelectMenu(
            CreateSelectMenu::new(SELECT_REMOVE, CreateSelectMenuKind::String { options })
                .placeholder("Remove a queued track")
                .min_values(1)
                .max_values(1),
        ));

        rows.push(CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                SELECT_REMOVE_RANGE,
                CreateSelectMenuKind::String {
                    options: remove_range_options(start, state.queue.len()),
                },
            )
            .placeholder("Remove a range")
            .min_values(1)
            .max_values(1),
        ));
    }

    rows
}

pub fn clear_confirm_buttons() -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BTN_CLEAR_CONFIRM)
            .label("Confirm Clear")
            .style(ButtonStyle::Danger),
        CreateButton::new(BTN_CLEAR_CANCEL)
            .label("Cancel")
            .style(ButtonStyle::Secondary),
    ])]
}

fn remove_range_options(start: usize, total: usize) -> Vec<CreateSelectMenuOption> {
    let visible_start = start + 1;
    let visible_end = (start + PAGE_SIZE).min(total);
    let mut options = vec![CreateSelectMenuOption::new(
        format!("Visible page ({visible_start}-{visible_end})"),
        format!("{visible_start}:{visible_end}"),
    )
    .description("Remove every track visible on this page")];

    let mut range_start = visible_start;
    while range_start <= visible_end && options.len() < 25 {
        let range_end = (range_start + 4).min(visible_end);
        options.push(
            CreateSelectMenuOption::new(
                format!("{range_start}-{range_end}"),
                format!("{range_start}:{range_end}"),
            )
            .description("Remove this range"),
        );
        range_start = range_end + 1;
    }

    options
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

pub async fn send_or_update_queue_panel(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) -> Result<(), Error> {
    let embed = build_queue_embed(data, guild_id).await;
    let components = build_queue_buttons(data, guild_id).await;

    let maybe_msg_id = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        state.queue_message_id
    };

    if let Some(message_id) = maybe_msg_id {
        let edit = EditMessage::new().embed(embed).components(components);

        match channel_id.edit_message(ctx, message_id, edit).await {
            Ok(_) => return Ok(()),
            Err(err) => tracing::warn!("failed edit queue panel, send new one: {err:?}"),
        }
    }

    let msg = channel_id
        .send_message(
            ctx,
            CreateMessage::new()
                .embed(build_queue_embed(data, guild_id).await)
                .components(build_queue_buttons(data, guild_id).await),
        )
        .await?;

    {
        let state_lock = data.music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue_message_id = Some(msg.id);
        state.queue_channel_id = Some(channel_id);
    }

    Ok(())
}

pub async fn update_queue_message(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let channel_id = {
        let state_lock = data.music.get(guild_id).await;
        let state = state_lock.lock().await;
        state.queue_channel_id
    };

    if let Some(channel_id) = channel_id {
        send_or_update_queue_panel(ctx, data, guild_id, channel_id).await?;
    }

    Ok(())
}
