use poise::serenity_prelude as serenity;

use crate::{permissions, Ctx, Error};

/// Kelola setting music bot server.
#[poise::command(
    slash_command,
    subcommands(
        "show",
        "cooldown",
        "maxqueue",
        "voteskip",
        "normalize_cap",
        "default_volume",
        "idle_timeout",
        "reset",
        "allow_channel",
        "unallow_channel",
        "allowed_channels",
        "block",
        "unblock",
        "blocklist"
    ),
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
        "Music config:\n- Default volume: `{}%`\n- Cooldown: `{}` detik\n- Max queue per user: `{}` lagu\n- Vote skip threshold: `{}%`\n- Normalize cap: `{}%`\n- Idle timeout: `{}` detik\n- Allowed channels: `{}`\n- Blocked terms: `{}`",
        db.guild_volume(guild_id)?,
        db.play_cooldown_secs(guild_id)?,
        db.max_queue_per_user(guild_id)?,
        db.vote_skip_percent(guild_id)?,
        db.normalize_cap_percent(guild_id)?,
        db.idle_timeout_secs(guild_id)?,
        db.allowed_channels(guild_id)?.len(),
        db.blocked_terms(guild_id)?.len(),
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

/// Set default/current volume server.
#[poise::command(slash_command, rename = "default-volume")]
pub async fn default_volume(
    ctx: Ctx<'_>,
    #[description = "Volume 0 sampai 200 persen"] percent: u8,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let percent = percent.min(200);
    crate::music::player::set_volume(ctx.serenity_context(), ctx.data(), guild_id, percent).await?;
    ctx.say(format!("Default/current volume set to `{percent}%`."))
        .await?;

    Ok(())
}

/// Set idle timeout sebelum bot leave otomatis.
#[poise::command(slash_command, rename = "idle-timeout")]
pub async fn idle_timeout(
    ctx: Ctx<'_>,
    #[description = "Timeout 10 sampai 600 detik"] seconds: u64,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let seconds = seconds.clamp(10, 600);
    ctx.data().db.set_idle_timeout_secs(guild_id, seconds)?;
    ctx.say(format!("Idle timeout set to `{seconds}` detik."))
        .await?;

    Ok(())
}

/// Reset setting server ke default.
#[poise::command(slash_command)]
pub async fn reset(ctx: Ctx<'_>) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    ctx.data().db.reset_guild_settings(guild_id)?;
    ctx.say("Music config reset to defaults.").await?;

    Ok(())
}

/// Batasi music command ke channel tertentu.
#[poise::command(slash_command, rename = "allow-channel")]
pub async fn allow_channel(
    ctx: Ctx<'_>,
    #[description = "Channel yang diizinkan"] channel: serenity::GuildChannel,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    ctx.data().db.add_allowed_channel(guild_id, channel.id)?;
    ctx.say(format!(
        "Music bot sekarang boleh dipakai di <#{}>.",
        channel.id.get()
    ))
    .await?;

    Ok(())
}

/// Hapus channel dari allowlist music command.
#[poise::command(slash_command, rename = "unallow-channel")]
pub async fn unallow_channel(
    ctx: Ctx<'_>,
    #[description = "Channel yang mau dihapus"] channel: serenity::GuildChannel,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    if ctx.data().db.remove_allowed_channel(guild_id, channel.id)? {
        ctx.say(format!(
            "Channel <#{}> dihapus dari allowlist.",
            channel.id.get()
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Channel <#{}> belum ada di allowlist.",
            channel.id.get()
        ))
        .await?;
    }

    Ok(())
}

/// Lihat channel yang diizinkan untuk music command.
#[poise::command(slash_command, rename = "allowed-channels")]
pub async fn allowed_channels(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let channels = ctx.data().db.allowed_channels(guild_id)?;

    if channels.is_empty() {
        ctx.say("Belum ada allowed channel. Music command bisa dipakai di semua channel.")
            .await?;
    } else {
        let list = channels
            .iter()
            .map(|channel| format!("- <#{}>", channel.get()))
            .collect::<Vec<_>>()
            .join("\n");
        ctx.say(format!("Allowed music channels:\n{list}")).await?;
    }

    Ok(())
}

/// Block keyword atau URL supaya tidak bisa diputar.
#[poise::command(slash_command)]
pub async fn block(
    ctx: Ctx<'_>,
    #[description = "Keyword atau URL yang diblok"] term: String,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let term = normalize_term_input(&term)?;
    ctx.data().db.add_blocked_term(guild_id, &term)?;
    ctx.say(format!("Blocked `{term}` untuk server ini."))
        .await?;

    Ok(())
}

/// Hapus keyword atau URL dari blocklist.
#[poise::command(slash_command)]
pub async fn unblock(
    ctx: Ctx<'_>,
    #[description = "Keyword atau URL yang mau dihapus"] term: String,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let term = normalize_term_input(&term)?;
    if ctx.data().db.remove_blocked_term(guild_id, &term)? {
        ctx.say(format!("Unblocked `{term}`.")).await?;
    } else {
        ctx.say(format!("`{term}` belum ada di blocklist.")).await?;
    }

    Ok(())
}

/// Lihat blocklist server.
#[poise::command(slash_command)]
pub async fn blocklist(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let terms = ctx.data().db.blocked_terms(guild_id)?;

    if terms.is_empty() {
        ctx.say("Blocklist masih kosong.").await?;
    } else {
        let list = terms
            .iter()
            .map(|term| format!("- `{term}`"))
            .collect::<Vec<_>>()
            .join("\n");
        ctx.say(format!("Blocked terms:\n{list}")).await?;
    }

    Ok(())
}

fn normalize_term_input(term: &str) -> Result<String, Error> {
    let term = term.trim().to_lowercase();
    if term.is_empty() {
        return Err("Term blocklist tidak boleh kosong.".into());
    }
    if term.len() > 128 {
        return Err("Term blocklist maksimal 128 karakter.".into());
    }
    Ok(term)
}
