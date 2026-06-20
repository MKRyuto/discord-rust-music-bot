use crate::{permissions, ui::player_panel, Ctx, Error};

/// Enable atau disable FFmpeg loudness normalization.
#[poise::command(slash_command)]
pub async fn normalize(
    ctx: Ctx<'_>,
    #[description = "Nyalakan true loudness normalization"] enabled: bool,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    ctx.data().db.set_normalize_enabled(guild_id, enabled)?;
    player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
        .await
        .ok();

    let status = if enabled { "enabled" } else { "disabled" };
    ctx.say(format!(
        "Normalize `{status}`. FFmpeg loudnorm berlaku mulai track berikutnya atau setelah replay."
    ))
    .await?;

    Ok(())
}
