use std::time::Duration;

use poise::serenity_prelude as serenity;
use tokio::time::timeout;

use crate::{
    music::{player, track::Track},
    ui::player_panel,
    Ctx, Error,
};

pub async fn autocomplete_track(ctx: Ctx<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let Some(guild_id) = ctx.guild_id() else {
        return Vec::new();
    };

    let query = partial.trim();
    if query.starts_with("http://") || query.starts_with("https://") {
        return Vec::new();
    }

    match ctx.data().db.search_history(guild_id, query, 10) {
        Ok(tracks) => tracks
            .into_iter()
            .map(|track| {
                let label = format!(
                    "{} [{} play(s)]",
                    truncate_choice(&track.title, 82),
                    track.play_count
                );
                serenity::AutocompleteChoice::new(label, truncate_choice(&track.title, 100))
            })
            .collect(),
        Err(err) => {
            tracing::warn!("track autocomplete failed: {err:?}");
            Vec::new()
        }
    }
}

fn truncate_choice(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let take = max_chars.saturating_sub(3);
    let mut output = value.chars().take(take).collect::<String>();
    output.push_str("...");
    output
}

/// Play lagu dari YouTube URL atau keyword.
#[poise::command(slash_command)]
pub async fn play(
    ctx: Ctx<'_>,
    #[description = "YouTube URL atau keyword lagu"]
    #[autocomplete = "autocomplete_track"]
    query_or_url: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let channel_id = ctx.channel_id();
    let user_id = ctx.author().id;

    ctx.defer().await?;
    ctx.say("Lagi nyiapin lagu...").await.ok();

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
            tracing::warn!("yt-dlp metadata timed out for query: {query_or_url}");
            ctx.say("Metadata YouTube kelamaan, gua coba putar dari URL/query mentah.")
                .await
                .ok();
            Track::unknown(query_or_url, user_id)
        }
    };
    let title = track.title.clone();

    {
        let state_lock = ctx.data().music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue.push_back(track);
        state.player_channel_id = Some(channel_id);
    }
    player::persist_queue(ctx.data(), guild_id).await;

    if let Err(err) = player::start_if_idle(ctx.serenity_context(), ctx.data(), guild_id).await {
        ctx.say(format!(
            "Gagal mulai audio: {err}\nCek bot sudah punya permission voice, dan `yt-dlp` + `ffmpeg` ada di PATH."
        ))
        .await
        .ok();
        return Ok(());
    }

    ctx.say(format!("Added: **{title}**")).await?;

    if let Err(err) = player_panel::send_or_update_player_panel(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        channel_id,
    )
    .await
    {
        tracing::warn!("failed to send/update player panel: {err:?}");
        ctx.say(format!("Lagu masuk, tapi gagal kirim panel player: {err}"))
            .await
            .ok();
    }

    Ok(())
}
