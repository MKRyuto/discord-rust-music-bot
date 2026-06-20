use poise::serenity_prelude as serenity;

use crate::{
    music::{player, track::Track},
    permissions,
    ui::{player_panel, queue_panel},
    Ctx, Error,
};

/// Kelola saved playlist server ini.
#[poise::command(
    slash_command,
    subcommands("save", "load", "list", "delete"),
    subcommand_required
)]
pub async fn playlist(_ctx: Ctx<'_>) -> Result<(), Error> {
    Ok(())
}

pub async fn autocomplete_playlist(
    ctx: Ctx<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let Some(guild_id) = ctx.guild_id() else {
        return Vec::new();
    };

    match ctx.data().db.search_playlists(guild_id, partial, 10) {
        Ok(playlists) => playlists
            .into_iter()
            .map(|playlist| {
                serenity::AutocompleteChoice::new(
                    format!("{} [{} track(s)]", playlist.name, playlist.track_count),
                    playlist.name,
                )
            })
            .collect(),
        Err(err) => {
            tracing::warn!("playlist autocomplete failed: {err:?}");
            Vec::new()
        }
    }
}

/// Simpan now playing + queue sebagai playlist.
#[poise::command(slash_command)]
pub async fn save(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"] name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let name = normalize_name(&name)?;

    let tracks = {
        let state_lock = ctx.data().music.get(guild_id).await;
        let state = state_lock.lock().await;
        collect_playlist_tracks(&state.now_playing, &state.queue)
    };

    if tracks.is_empty() {
        ctx.say("Tidak ada lagu buat disimpan.").await?;
        return Ok(());
    }

    ctx.data()
        .db
        .save_playlist(guild_id, &name, ctx.author().id, &tracks)?;

    ctx.say(format!(
        "Saved playlist `{name}` with `{}` track(s).",
        tracks.len()
    ))
    .await?;

    Ok(())
}

/// Load playlist ke queue.
#[poise::command(slash_command)]
pub async fn load(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"]
    #[autocomplete = "autocomplete_playlist"]
    name: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let channel_id = ctx.channel_id();
    let user_id = ctx.author().id;
    let name = normalize_name(&name)?;
    let tracks = ctx
        .data()
        .db
        .load_playlist(guild_id, &name, ctx.author().id)?;

    if tracks.is_empty() {
        ctx.say(format!("Playlist `{name}` kosong atau tidak ditemukan."))
            .await?;
        return Ok(());
    }

    let max_queue_per_user = ctx.data().db.max_queue_per_user(guild_id)?;
    let queued_by_user = player::user_queue_count(ctx.data(), guild_id, user_id).await;
    let remaining_slots = max_queue_per_user.saturating_sub(queued_by_user);
    if remaining_slots == 0 {
        ctx.say(format!(
            "Queue lu sudah mencapai batas `{}` lagu. Hapus beberapa dulu sebelum load playlist.",
            max_queue_per_user
        ))
        .await?;
        return Ok(());
    }

    let tracks = tracks.into_iter().take(remaining_slots).collect::<Vec<_>>();

    if let Err(err) = player::join_user_channel(ctx.serenity_context(), guild_id, user_id).await {
        ctx.say(format!("Gagal join voice channel: {err}"))
            .await
            .ok();
        return Ok(());
    }

    {
        let state_lock = ctx.data().music.get(guild_id).await;
        let mut state = state_lock.lock().await;
        state.queue.extend(tracks.clone());
        state.player_channel_id = Some(channel_id);
    }
    player::persist_queue(ctx.data(), guild_id).await;

    player::start_if_idle(ctx.serenity_context(), ctx.data(), guild_id).await?;
    player_panel::send_or_update_player_panel(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        channel_id,
    )
    .await
    .ok();
    queue_panel::update_queue_message(ctx.serenity_context(), ctx.data(), guild_id)
        .await
        .ok();

    ctx.say(format!(
        "Loaded playlist `{name}` with `{}` track(s).",
        tracks.len()
    ))
    .await?;

    Ok(())
}

/// Lihat semua saved playlist.
#[poise::command(slash_command)]
pub async fn list(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let playlists = ctx.data().db.list_playlists(guild_id)?;

    if playlists.is_empty() {
        ctx.say("Belum ada saved playlist di server ini.").await?;
        return Ok(());
    }

    let desc = playlists
        .iter()
        .map(|playlist| {
            format!(
                "- `{}` - `{}` track(s)",
                playlist.name, playlist.track_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    ctx.say(format!("Saved playlists:\n{desc}")).await?;

    Ok(())
}

/// Hapus saved playlist.
#[poise::command(slash_command)]
pub async fn delete(
    ctx: Ctx<'_>,
    #[description = "Nama playlist"]
    #[autocomplete = "autocomplete_playlist"]
    name: String,
) -> Result<(), Error> {
    if !permissions::require_music_control(ctx).await? {
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;
    let name = normalize_name(&name)?;

    if ctx.data().db.delete_playlist(guild_id, &name)? {
        ctx.say(format!("Deleted playlist `{name}`.")).await?;
    } else {
        ctx.say(format!("Playlist `{name}` tidak ditemukan."))
            .await?;
    }

    Ok(())
}

fn collect_playlist_tracks(
    now_playing: &Option<Track>,
    queue: &std::collections::VecDeque<Track>,
) -> Vec<Track> {
    now_playing
        .iter()
        .chain(queue.iter())
        .cloned()
        .collect::<Vec<_>>()
}

fn normalize_name(name: &str) -> Result<String, Error> {
    let name = name.trim();

    if name.is_empty() {
        return Err("Nama playlist tidak boleh kosong.".into());
    }

    if name.len() > 64 {
        return Err("Nama playlist maksimal 64 karakter.".into());
    }

    Ok(name.to_string())
}
