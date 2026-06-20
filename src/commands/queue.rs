use crate::{
    music::player,
    permissions,
    ui::{player_panel, queue_panel},
    Ctx, Error,
};

/// Tampilkan queue musik.
#[poise::command(
    slash_command,
    subcommands(
        "show",
        "clear",
        "remove",
        "remove_search",
        "remove_range",
        "mine",
        "remove_mine",
        "jump",
        "move_track"
    )
)]
pub async fn queue(ctx: Ctx<'_>) -> Result<(), Error> {
    show_queue_panel(ctx).await
}

/// Tampilkan queue musik.
#[poise::command(slash_command)]
pub async fn show(ctx: Ctx<'_>) -> Result<(), Error> {
    show_queue_panel(ctx).await
}

async fn show_queue_panel(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    queue_panel::send_or_update_queue_panel(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        ctx.channel_id(),
    )
    .await?;

    Ok(())
}

/// Bersihkan queue.
#[poise::command(slash_command)]
pub async fn clear(ctx: Ctx<'_>) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let cleared = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let cleared = state.queue.len();
        state.queue.clear();
        state.queue_page = 0;
        cleared
    };

    player::persist_queue(ctx.data(), guild_id).await;
    queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
        .await
        .ok();
    player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
        .await
        .ok();

    ctx.say(format!("Cleared `{cleared}` queued track(s)."))
        .await?;

    Ok(())
}

/// Lihat lagu yang lu request di queue.
#[poise::command(slash_command)]
pub async fn mine(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let user_id = ctx.author().id;

    let tracks = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let state = state_lock.lock().await;
        state
            .queue
            .iter()
            .enumerate()
            .filter(|(_, track)| track.requested_by == user_id)
            .take(15)
            .map(|(idx, track)| format!("`{}.` **{}**", idx + 1, track.title))
            .collect::<Vec<_>>()
    };

    if tracks.is_empty() {
        ctx.say("Lu belum punya lagu di queue.").await?;
    } else {
        ctx.say(format!("Your queued tracks:\n{}", tracks.join("\n")))
            .await?;
    }

    Ok(())
}

/// Hapus semua lagu yang lu request dari queue.
#[poise::command(slash_command, rename = "remove-mine")]
pub async fn remove_mine(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let user_id = ctx.author().id;

    let removed = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        let before = state.queue.len();
        state.queue.retain(|track| track.requested_by != user_id);
        let removed = before - state.queue.len();
        if removed > 0 {
            state.queue_page = 0;
        }
        removed
    };

    if removed > 0 {
        player::persist_queue(ctx.data(), guild_id).await;
        queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
    }

    ctx.say(format!("Removed `{removed}` of your queued track(s)."))
        .await?;

    Ok(())
}

/// Hapus lagu dari queue berdasarkan nomor.
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Ctx<'_>,
    #[description = "Nomor lagu di queue"] position: usize,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    match player::remove_queued_track(ctx.data(), guild_id, position).await {
        Some(track) => {
            queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
                .await
                .ok();
            player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
                .await
                .ok();
            ctx.say(format!("Removed `{position}.` **{}**", track.title))
                .await?;
        }
        None => {
            ctx.say(format!("Queue position `{position}` tidak ditemukan."))
                .await?;
        }
    }

    Ok(())
}

/// Hapus lagu dari queue berdasarkan judul atau URL.
#[poise::command(slash_command, rename = "remove-search")]
pub async fn remove_search(
    ctx: Ctx<'_>,
    #[description = "Sebagian judul atau URL lagu"] query: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    match player::remove_queued_track_matching(ctx.data(), guild_id, &query).await {
        Some((position, track)) => {
            queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
                .await
                .ok();
            player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
                .await
                .ok();
            ctx.say(format!("Removed `{position}.` **{}**", track.title))
                .await?;
        }
        None => {
            ctx.say(format!("Tidak nemu queue match buat `{query}`."))
                .await?;
        }
    }

    Ok(())
}

/// Hapus beberapa lagu dari queue berdasarkan range nomor.
#[poise::command(slash_command, rename = "remove-range")]
pub async fn remove_range(
    ctx: Ctx<'_>,
    #[description = "Nomor awal"] start: usize,
    #[description = "Nomor akhir"] end: usize,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let removed = player::remove_queued_track_range(ctx.data(), guild_id, start, end).await;

    if removed.is_empty() {
        ctx.say("Range queue tidak valid atau kosong.").await?;
    } else {
        queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        ctx.say(format!("Removed `{}` queued track(s).", removed.len()))
            .await?;
    }

    Ok(())
}

/// Lompat ke halaman queue tertentu.
#[poise::command(slash_command)]
pub async fn jump(ctx: Ctx<'_>, #[description = "Nomor halaman"] page: usize) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    match player::set_queue_page(ctx.data(), guild_id, page).await {
        Some((current, total)) => {
            queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
                .await
                .ok();
            ctx.say(format!("Queue page set to `{current}/{total}`."))
                .await?;
        }
        None => {
            ctx.say("Nomor halaman tidak valid.").await?;
        }
    }

    Ok(())
}

/// Pindah urutan lagu di queue.
#[poise::command(slash_command, rename = "move")]
pub async fn move_track(
    ctx: Ctx<'_>,
    #[description = "Nomor lagu awal"] from: usize,
    #[description = "Nomor tujuan"] to: usize,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    if player::move_queued_track(ctx.data(), guild_id, from, to).await {
        queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        ctx.say(format!("Moved queue track `{from}` to `{to}`."))
            .await?;
    } else {
        ctx.say("Nomor queue tidak valid.").await?;
    }

    Ok(())
}
