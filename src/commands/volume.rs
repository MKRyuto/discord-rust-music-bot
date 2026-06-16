use crate::{music::player, Ctx, Error};

/// Set volume musik server ini.
#[poise::command(slash_command)]
pub async fn volume(
    ctx: Ctx<'_>,
    #[description = "Volume 0 sampai 200 persen"] percent: u8,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let percent = percent.min(200);

    player::set_volume(ctx.serenity_context(), ctx.data(), guild_id, percent).await?;
    ctx.say(format!("Volume set to `{percent}%`.")).await?;

    Ok(())
}
