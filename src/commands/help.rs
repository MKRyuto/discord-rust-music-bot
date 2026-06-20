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
            "`/play`, `/playnow`, `/replay`, `/previous`, `/seek`, `/now`, `/voteskip`, `/leave`",
            false,
        )
        .field(
            "Queue",
            "`/queue show`, `/queue mine`, `/queue remove-mine`, `/queue clear`, `/queue remove`, `/queue remove-search`, `/queue remove-range`, `/queue jump`, `/queue move`, `/shuffle`",
            false,
        )
        .field(
            "Settings",
            "`/volume`, `/autoplay`, `/normalize`, `/config show`, `/config cooldown`, `/config maxqueue`, `/config voteskip`, `/config normalize-cap`, `/config default-volume`, `/config idle-timeout`, `/config reset`, `/config allow-channel`, `/config block`, `/djrole add`, `/djrole remove`, `/djrole list`",
            false,
        )
        .field(
            "Library",
            "`/playlist save`, `/playlist append`, `/playlist load`, `/playlist rename`, `/playlist list`, `/playlist delete`, `/history`, `/stats server`, `/stats user`",
            false,
        )
        .field(
            "Rules",
            "Control command bisa dibatasi pakai DJ role. Cooldown, max queue, vote skip threshold, dan normalize cap bisa diatur pakai `/config`.",
            false,
        );

    ctx.send(CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}
