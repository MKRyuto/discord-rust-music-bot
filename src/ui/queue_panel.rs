use poise::serenity_prelude as serenity;
use serenity::{ButtonStyle, CreateActionRow, CreateButton, CreateEmbed};

use crate::Data;

pub const BTN_PREV: &str = "music:queue_prev";
pub const BTN_NEXT: &str = "music:queue_next";
pub const BTN_CLEAR: &str = "music:queue_clear";
pub const BTN_PLAYER: &str = "music:player";

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

    vec![CreateActionRow::Buttons(vec![
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
            .style(ButtonStyle::Danger),
        CreateButton::new(BTN_PLAYER)
            .label("Player")
            .style(ButtonStyle::Primary),
    ])]
}
