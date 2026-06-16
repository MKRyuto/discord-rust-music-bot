use poise::CreateReply;

use crate::{ui::player_panel, Ctx, Error};

/// Tampilkan player panel.
#[poise::command(slash_command)]
pub async fn now(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let embed = player_panel::build_player_embed(ctx.data(), guild_id).await;
    let components = player_panel::build_player_components(ctx.data(), guild_id).await;

    ctx.send(CreateReply::default().embed(embed).components(components))
        .await?;

    Ok(())
}
