use crate::{
    music::player,
    permissions,
    ui::{player_panel, queue_panel},
    Ctx, Error,
};

/// Tampilkan queue musik.
#[poise::command(
    slash_command,
    subcommands("show", "clear", "remove", "remove_search", "move_track")
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
