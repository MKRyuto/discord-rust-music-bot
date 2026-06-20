use poise::{serenity_prelude as serenity, CreateReply};

use crate::{Ctx, Error};

/// Tampilkan bantuan command music bot.
#[poise::command(slash_command)]
pub async fn help(ctx: Ctx<'_>) -> Result<(), Error> {
    let embed = serenity::CreateEmbed::new()
        .title("Music Bot Help")
        .description("Slash command utama buat playback, queue, playlist, dan server control.")
        .field(
            "Playback",
            "`/play`, `/playnow`, `/now`, `/voteskip`, `/leave`",
            false,
        )
        .field(
            "Queue",
            "`/queue show`, `/queue clear`, `/queue remove`, `/queue remove-search`, `/queue move`, `/shuffle`",
            false,
        )
        .field(
            "Settings",
            "`/volume`, `/autoplay`, `/normalize`, `/djrole add`, `/djrole remove`, `/djrole list`",
            false,
        )
        .field(
            "Library",
            "`/playlist save`, `/playlist load`, `/playlist list`, `/playlist delete`, `/history`",
            false,
        )
        .field(
            "Rules",
            "Control command bisa dibatasi pakai DJ role. `/play` punya cooldown 10 detik dan batas 10 lagu per user di queue.",
            false,
        );

    ctx.send(CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}
