use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{music::track::Track, ui::player_panel, Ctx, Error};

/// Acak urutan queue.
#[poise::command(slash_command)]
pub async fn shuffle(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let total = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let mut state = state_lock.lock().await;

        if state.queue.len() < 2 {
            0
        } else {
            let mut tracks = state.queue.drain(..).collect::<Vec<_>>();
            shuffle_tracks(&mut tracks);
            let total = tracks.len();
            state.queue = VecDeque::from(tracks);
            state.queue_page = 0;
            total
        }
    };

    if total == 0 {
        ctx.say("Queue butuh minimal 2 lagu buat di-shuffle.").await?;
    } else {
        player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        ctx.say(format!("Shuffled `{total}` queued track(s).")).await?;
    }

    Ok(())
}

fn shuffle_tracks(tracks: &mut [Track]) {
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15);

    for idx in (1..tracks.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let swap_idx = (seed as usize) % (idx + 1);
        tracks.swap(idx, swap_idx);
    }
}
