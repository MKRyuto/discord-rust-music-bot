use crate::{music::player, permissions, Ctx, Error};

/// Putar ulang lagu sekarang dari awal.
#[poise::command(slash_command)]
pub async fn replay(ctx: Ctx<'_>) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    player::replay(ctx.serenity_context(), ctx.data(), guild_id).await?;
    ctx.say("Replaying current track.").await?;

    Ok(())
}
