use poise::CreateReply;

use crate::{ui::queue_panel, Ctx, Error};

/// Tampilkan queue musik.
#[poise::command(slash_command)]
pub async fn queue(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let embed = queue_panel::build_queue_embed(ctx.data(), guild_id).await;

    ctx.send(
        CreateReply::default()
            .embed(embed)
            .components(queue_panel::build_queue_buttons(ctx.data(), guild_id).await),
    )
    .await?;

    Ok(())
}
