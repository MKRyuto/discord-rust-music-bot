use crate::{music::player, ui::player_panel, Ctx, Error};

/// Acak urutan queue.
#[poise::command(slash_command)]
pub async fn shuffle(ctx: Ctx<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("Command ini cuma bisa dipakai di server.")?;

    let total = player::shuffle_queue(ctx.data(), guild_id).await;

    if total == 0 {
        ctx.say("Queue butuh minimal 2 lagu buat di-shuffle.")
            .await?;
    } else {
        player_panel::update_player_message(ctx.serenity_context(), ctx.data(), guild_id)
            .await
            .ok();
        ctx.say(format!("Shuffled `{total}` queued track(s)."))
            .await?;
    }

    Ok(())
}
