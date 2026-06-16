use crate::{Ctx, Error};

/// Lihat lagu yang paling sering diputar di server ini.
#[poise::command(slash_command)]
pub async fn history(
    ctx: Ctx<'_>,
    #[description = "Jumlah lagu yang ditampilkan, maksimal 20"] limit: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let limit = limit.unwrap_or(10).clamp(1, 20);
    let tracks = ctx.data().db.top_history(guild_id, limit)?;

    if tracks.is_empty() {
        ctx.say("Belum ada history lagu di server ini. Putar beberapa lagu dulu.")
            .await?;
        return Ok(());
    }

    let desc = tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            format!(
                "`{}.` **{}** - `{}` play(s)",
                idx + 1,
                track.title,
                track.play_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    ctx.say(format!("Top played tracks:\n{desc}")).await?;

    Ok(())
}
