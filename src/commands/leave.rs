use crate::{music::player, permissions, Ctx, Error};

/// Stop musik dan keluar dari voice channel.
#[poise::command(slash_command)]
pub async fn leave(ctx: Ctx<'_>) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    player::leave(ctx.serenity_context(), ctx.data(), guild_id).await?;
    ctx.say("Bot keluar dari voice channel.").await?;

    Ok(())
}
