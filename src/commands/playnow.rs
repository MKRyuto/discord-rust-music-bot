use std::time::Duration;

use tokio::time::timeout;

use crate::{
    commands::play::autocomplete_track,
    music::{player, track::Track},
    Ctx, Error,
};

/// Play lagu sekarang dan biarkan queue lama tetap lanjut setelahnya.
#[poise::command(slash_command)]
pub async fn playnow(
    ctx: Ctx<'_>,
    #[description = "YouTube URL atau keyword lagu"]
    #[autocomplete = "autocomplete_track"]
    query_or_url: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let user_id = ctx.author().id;

    ctx.defer().await?;
    ctx.say("Lagi nyiapin lagu buat diputar sekarang...")
        .await
        .ok();

    if let Err(err) = player::join_user_channel(ctx.serenity_context(), guild_id, user_id).await {
        ctx.say(format!("Gagal join voice channel: {err}"))
            .await
            .ok();
        return Ok(());
    }

    let track = match timeout(
        Duration::from_secs(15),
        player::resolve_track(ctx.data(), query_or_url.clone(), user_id),
    )
    .await
    {
        Ok(track) => track,
        Err(_) => {
            tracing::warn!("yt-dlp metadata timed out for playnow query: {query_or_url}");
            ctx.say("Metadata YouTube kelamaan, gua coba putar dari URL/query mentah.")
                .await
                .ok();
            Track::unknown(query_or_url, user_id)
        }
    };
    let title = track.title.clone();

    if let Err(err) = player::play_now(ctx.serenity_context(), ctx.data(), guild_id, track).await {
        ctx.say(format!(
            "Gagal play now: {err}\nCek bot sudah punya permission voice, dan `yt-dlp` + `ffmpeg` ada di PATH."
        ))
        .await
        .ok();
        return Ok(());
    }

    ctx.say(format!("Playing now: **{title}**")).await?;

    Ok(())
}
