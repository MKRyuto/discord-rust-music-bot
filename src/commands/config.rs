use crate::{permissions, Ctx, Error};

/// Kelola setting music bot server.
#[poise::command(
    slash_command,
    subcommands("show", "cooldown", "maxqueue", "voteskip", "normalize_cap"),
    subcommand_required
)]
pub async fn config(_ctx: Ctx<'_>) -> Result<(), Error> {
    Ok(())
}

/// Lihat setting music bot server.
#[poise::command(slash_command)]
pub async fn show(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let db = &ctx.data().db;
    ctx.say(format!(
        "Music config:\n- Cooldown: `{}` detik\n- Max queue per user: `{}` lagu\n- Vote skip threshold: `{}%`\n- Normalize cap: `{}%`",
        db.play_cooldown_secs(guild_id)?,
        db.max_queue_per_user(guild_id)?,
        db.vote_skip_percent(guild_id)?,
        db.normalize_cap_percent(guild_id)?,
    ))
    .await?;

    Ok(())
}

/// Set cooldown /play per user.
#[poise::command(slash_command)]
pub async fn cooldown(
    ctx: Ctx<'_>,
    #[description = "Cooldown 0 sampai 300 detik"] seconds: u64,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let seconds = seconds.min(300);
    ctx.data().db.set_play_cooldown_secs(guild_id, seconds)?;
    ctx.say(format!("Play cooldown set to `{seconds}` detik."))
        .await?;

    Ok(())
}

/// Set max lagu aktif per user.
#[poise::command(slash_command)]
pub async fn maxqueue(
    ctx: Ctx<'_>,
    #[description = "Limit 1 sampai 100 lagu"] limit: usize,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let limit = limit.clamp(1, 100);
    ctx.data().db.set_max_queue_per_user(guild_id, limit)?;
    ctx.say(format!("Max queue per user set to `{limit}` lagu."))
        .await?;

    Ok(())
}

/// Set persentase vote skip.
#[poise::command(slash_command)]
pub async fn voteskip(
    ctx: Ctx<'_>,
    #[description = "Threshold 1 sampai 100 persen"] percent: u8,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let percent = percent.clamp(1, 100);
    ctx.data().db.set_vote_skip_percent(guild_id, percent)?;
    ctx.say(format!("Vote skip threshold set to `{percent}%`."))
        .await?;

    Ok(())
}

/// Set batas volume efektif saat normalize ON.
#[poise::command(slash_command, rename = "normalize-cap")]
pub async fn normalize_cap(
    ctx: Ctx<'_>,
    #[description = "Cap 1 sampai 200 persen"] percent: u8,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let percent = percent.clamp(1, 200);
    ctx.data().db.set_normalize_cap_percent(guild_id, percent)?;
    ctx.say(format!("Normalize cap set to `{percent}%`."))
        .await?;

    Ok(())
}
