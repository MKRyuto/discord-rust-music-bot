use crate::{music::player, Ctx, Error};

/// Vote buat skip lagu sekarang.
#[poise::command(slash_command)]
pub async fn voteskip(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let (votes, needed, skipped) = player::vote_skip(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        ctx.author().id,
    )
    .await?;

    if skipped {
        ctx.say(format!("Vote skip lolos `{votes}/{needed}`. Skipping."))
            .await?;
    } else {
        ctx.say(format!("Vote skip: `{votes}/{needed}` vote(s)."))
            .await?;
    }

    Ok(())
}
