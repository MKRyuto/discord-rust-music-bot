use poise::serenity_prelude as serenity;

use crate::{permissions, Ctx, Error};

/// Kelola role DJ yang boleh mengatur music bot.
#[poise::command(
    slash_command,
    subcommands("add", "remove", "list"),
    subcommand_required
)]
pub async fn djrole(_ctx: Ctx<'_>) -> Result<(), Error> {
    Ok(())
}

/// Tambahkan role DJ.
#[poise::command(slash_command)]
pub async fn add(
    ctx: Ctx<'_>,
    #[description = "Role yang boleh mengatur music bot"] role: serenity::Role,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    ctx.data().db.add_dj_role(guild_id, role.id)?;
    ctx.say(format!(
        "Role <@&{}> sekarang boleh mengatur music bot.",
        role.id.get()
    ))
    .await?;

    Ok(())
}

/// Hapus role DJ.
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Ctx<'_>,
    #[description = "Role DJ yang mau dihapus"] role: serenity::Role,
) -> Result<(), Error> {
    if !permissions::require_music_setup(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    if ctx.data().db.remove_dj_role(guild_id, role.id)? {
        ctx.say(format!(
            "Role <@&{}> dihapus dari daftar DJ.",
            role.id.get()
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Role <@&{}> belum ada di daftar DJ.",
            role.id.get()
        ))
        .await?;
    }

    Ok(())
}

/// Lihat role DJ server ini.
#[poise::command(slash_command)]
pub async fn list(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let roles = ctx.data().db.dj_roles(guild_id)?;

    if roles.is_empty() {
        ctx.say("Belum ada role DJ. Saat kosong, semua member boleh pakai kontrol music.")
            .await?;
    } else {
        let list = roles
            .iter()
            .map(|role| format!("- <@&{}>", role.get()))
            .collect::<Vec<_>>()
            .join("\n");

        ctx.say(format!("Role DJ server ini:\n{list}")).await?;
    }

    Ok(())
}
