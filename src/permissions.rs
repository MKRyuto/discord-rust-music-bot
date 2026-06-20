use poise::serenity_prelude as serenity;
use serenity::Permissions;

use crate::{Ctx, Data, Error};

const DENIED_MESSAGE: &str =
    "Command ini cuma bisa dipakai admin server atau role DJ yang sudah diset.";
const SETUP_DENIED_MESSAGE: &str =
    "Command ini cuma bisa dipakai user dengan permission Manage Server atau Administrator.";

pub async fn require_music_control(ctx: Ctx<'_>) -> Result<bool, Error> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.say("Command ini cuma bisa dipakai di server.").await?;
        return Ok(false);
    };

    if !allowed_in_channel(ctx, guild_id).await? {
        return Ok(false);
    }

    let Some(member) = ctx.author_member().await else {
        ctx.say(DENIED_MESSAGE).await?;
        return Ok(false);
    };

    if can_control_music(ctx.serenity_context(), ctx.data(), guild_id, &member).await? {
        Ok(true)
    } else {
        ctx.say(DENIED_MESSAGE).await?;
        Ok(false)
    }
}

pub async fn require_allowed_channel(ctx: Ctx<'_>) -> Result<bool, Error> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.say("Command ini cuma bisa dipakai di server.").await?;
        return Ok(false);
    };

    allowed_in_channel(ctx, guild_id).await
}

async fn allowed_in_channel(ctx: Ctx<'_>, guild_id: serenity::GuildId) -> Result<bool, Error> {
    let allowed_channels = ctx.data().db.allowed_channels(guild_id)?;
    if allowed_channels.is_empty() || allowed_channels.contains(&ctx.channel_id()) {
        return Ok(true);
    }

    let list = allowed_channels
        .iter()
        .map(|channel| format!("<#{}>", channel.get()))
        .collect::<Vec<_>>()
        .join(", ");
    ctx.say(format!("Music bot cuma boleh dipakai di channel: {list}"))
        .await?;
    Ok(false)
}

pub async fn require_music_setup(ctx: Ctx<'_>) -> Result<bool, Error> {
    let Some(guild_id) = ctx.guild_id() else {
        ctx.say("Command ini cuma bisa dipakai di server.").await?;
        return Ok(false);
    };

    let Some(member) = ctx.author_member().await else {
        ctx.say(SETUP_DENIED_MESSAGE).await?;
        return Ok(false);
    };

    if has_guild_management(ctx.serenity_context(), guild_id, &member) {
        Ok(true)
    } else {
        ctx.say(SETUP_DENIED_MESSAGE).await?;
        Ok(false)
    }
}

pub async fn can_control_music(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    member: &serenity::Member,
) -> Result<bool, Error> {
    if has_guild_management(ctx, guild_id, member) {
        return Ok(true);
    }

    let dj_roles = data.db.dj_roles(guild_id)?;
    if dj_roles.is_empty() {
        return Ok(true);
    }

    Ok(member.roles.iter().any(|role| dj_roles.contains(role)))
}

pub fn has_guild_management(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    member: &serenity::Member,
) -> bool {
    let permissions = member.permissions.or_else(|| {
        guild_id
            .to_guild_cached(ctx)
            .map(|guild| guild.member_permissions(member))
    });

    if permissions.is_some_and(|permissions| {
        permissions.contains(Permissions::ADMINISTRATOR)
            || permissions.contains(Permissions::MANAGE_GUILD)
    }) {
        return true;
    }

    guild_id
        .to_guild_cached(ctx)
        .is_some_and(|guild| guild.owner_id == member.user.id)
}
