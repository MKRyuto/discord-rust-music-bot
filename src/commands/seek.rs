use std::time::Duration;

use crate::{music::player, permissions, Ctx, Error};

/// Seek lagu sekarang ke posisi tertentu.
#[poise::command(slash_command)]
pub async fn seek(
    ctx: Ctx<'_>,
    #[description = "Posisi, contoh 90, 1:30, atau 01:02:03"] position: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let position = parse_position(&position)?;

    player::seek(ctx.serenity_context(), ctx.data(), guild_id, position).await?;
    ctx.say(format!("Seeked to `{}`.", format_duration(position)))
        .await?;

    Ok(())
}

fn parse_position(raw: &str) -> Result<Duration, Error> {
    let parts = raw
        .trim()
        .split(':')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()?;

    let seconds = match parts.as_slice() {
        [seconds] => *seconds,
        [minutes, seconds] => minutes * 60 + seconds,
        [hours, minutes, seconds] => hours * 3600 + minutes * 60 + seconds,
        _ => return Err("Format seek harus `seconds`, `mm:ss`, atau `hh:mm:ss`.".into()),
    };

    Ok(Duration::from_secs(seconds))
}

fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}
