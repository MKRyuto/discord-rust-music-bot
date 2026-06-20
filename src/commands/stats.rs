use poise::serenity_prelude as serenity;

use crate::{Ctx, Error};

/// Lihat statistik music bot server.
#[poise::command(slash_command, subcommands("server", "user"), subcommand_required)]
pub async fn stats(_ctx: Ctx<'_>) -> Result<(), Error> {
    Ok(())
}

/// Lihat statistik server.
#[poise::command(slash_command)]
pub async fn server(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let stats = ctx.data().db.server_stats(guild_id)?;

    ctx.say(format!(
        "Server music stats:\n- Total plays: `{}`\n- Unique tracks: `{}`\n- Saved playlists: `{}`",
        stats.total_plays, stats.unique_tracks, stats.playlists
    ))
    .await?;

    Ok(())
}

/// Lihat statistik user.
#[poise::command(slash_command)]
pub async fn user(
    ctx: Ctx<'_>,
    #[description = "User yang mau dicek"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let user_id = user.as_ref().map(|user| user.id).unwrap_or(ctx.author().id);
    let stats = ctx.data().db.user_stats(guild_id, user_id)?;

    ctx.say(format!(
        "User music stats for <@{}>:\n- Tracks played: `{}`",
        stats.user_id.get(),
        stats.tracks_played
    ))
    .await?;

    Ok(())
}
