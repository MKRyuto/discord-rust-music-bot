use poise::CreateReply;

use crate::{ui::help_panel, Ctx, Error};

/// Tampilkan bantuan command music bot.
#[poise::command(slash_command)]
pub async fn help(ctx: Ctx<'_>) -> Result<(), Error> {
    ctx.send(
        CreateReply::default()
            .embed(help_panel::overview_embed())
            .components(help_panel::category_select(Some("overview")))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
