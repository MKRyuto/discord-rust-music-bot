use crate::{Ctx, Error};

/// Enable atau disable autoplay dari history server.
#[poise::command(slash_command)]
pub async fn autoplay(
    ctx: Ctx<'_>,
    #[description = "Nyalakan autoplay history-based"] enabled: bool,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    ctx.data().db.set_autoplay_enabled(guild_id, enabled)?;

    let status = if enabled { "enabled" } else { "disabled" };
    ctx.say(format!(
        "Autoplay `{status}`. Kalau queue habis, bot akan ambil lagu random dari history server."
    ))
    .await?;

    Ok(())
}
