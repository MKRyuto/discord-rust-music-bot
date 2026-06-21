mod commands;
mod interactions;
mod music;
mod permissions;
mod storage;
mod ui;
mod web;

use std::{env, path::PathBuf, sync::Arc, time::Duration};

use poise::serenity_prelude as serenity;
use serenity::{ActivityData, ClientBuilder, GatewayIntents};
use songbird::SerenityInit;

use crate::music::state::MusicStore;
use crate::storage::Database;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Ctx<'a> = poise::Context<'a, Data, Error>;

#[derive(Clone)]
pub struct Data {
    pub music: Arc<MusicStore>,
    pub http_client: reqwest::Client,
    pub db: Arc<Database>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let db_path = env::var("MUSIC_DB_PATH").unwrap_or_else(|_| "music_bot.db".to_string());
    let db = Arc::new(Database::new(db_path)?);
    tracing::info!("Using music database at {}", db.path().display());
    spawn_database_backups(db.clone());

    if env::var("WEB_PREVIEW")
        .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
        .unwrap_or(false)
    {
        let data = Data {
            music: Arc::new(MusicStore::default()),
            http_client: reqwest::Client::new(),
            db,
        };
        web::spawn(
            data,
            Arc::new(serenity::Cache::new()),
            None,
            web::BotProfile::preview(),
        )?;
        tracing::info!("WEB_PREVIEW enabled; Discord client is not started");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    let token = env::var("DISCORD_TOKEN")
        .expect("DISCORD_TOKEN belum diisi. Copy .env.example ke .env lalu isi token bot.");

    let intents = GatewayIntents::non_privileged() | GatewayIntents::GUILD_VOICE_STATES;

    let options = poise::FrameworkOptions {
        commands: vec![
            commands::play::play(),
            commands::playnow::playnow(),
            commands::replay::replay(),
            commands::previous::previous(),
            commands::seek::seek(),
            commands::voteskip::voteskip(),
            commands::history::history(),
            commands::stats::stats(),
            commands::help::help(),
            commands::config::config(),
            commands::queue::queue(),
            commands::now::now(),
            commands::leave::leave(),
            commands::autoplay::autoplay(),
            commands::normalize::normalize(),
            commands::djrole::djrole(),
            commands::volume::volume(),
            commands::shuffle::shuffle(),
            commands::playlist::playlist(),
        ],
        event_handler: |ctx, event, _framework, data| {
            Box::pin(async move {
                interactions::buttons::handle_event(ctx, event, data).await?;
                Ok(())
            })
        },
        pre_command: |ctx| {
            Box::pin(async move {
                tracing::info!(
                    command = %ctx.command().qualified_name,
                    guild_id = ?ctx.guild_id(),
                    "handling Discord command"
                );
            })
        },
        on_error: |error| Box::pin(handle_framework_error(error)),
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .options(options)
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                if let Ok(guild_id_raw) = env::var("DEV_GUILD_ID") {
                    if !guild_id_raw.trim().is_empty() {
                        let guild_id = serenity::GuildId::new(guild_id_raw.parse::<u64>()?);
                        poise::builtins::register_in_guild(
                            ctx,
                            &framework.options().commands,
                            guild_id,
                        )
                        .await?;
                        tracing::info!("Registered slash commands in DEV_GUILD_ID={guild_id}");
                    } else {
                        poise::builtins::register_globally(ctx, &framework.options().commands)
                            .await?;
                        tracing::info!("Registered slash commands globally");
                    }
                } else {
                    poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                    tracing::info!("Registered slash commands globally");
                }

                ctx.set_activity(Some(ActivityData::listening("/help | /play")));

                let music = Arc::new(MusicStore::default());
                for (guild_id, (now_playing, queue)) in db.load_all_queues()? {
                    let restored_count = queue.len() + usize::from(now_playing.is_some());
                    music.restore_queue(guild_id, now_playing, queue).await;
                    tracing::info!(guild_id = %guild_id, restored_count, "restored persisted queue");
                }

                let data = Data {
                    music,
                    http_client: reqwest::Client::new(),
                    db,
                };

                web::spawn(
                    data.clone(),
                    ctx.cache.clone(),
                    Some(Arc::new(ctx.clone())),
                    web::BotProfile::from_ready(ready),
                )?;

                Ok(data)
            })
        })
        .build();

    let mut client = ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird_from_config(
            songbird::Config::default()
                .gateway_timeout(Some(Duration::from_secs(20)))
                .driver_timeout(Some(Duration::from_secs(20))),
        )
        .await?;

    client.start().await?;

    Ok(())
}

async fn handle_framework_error(error: poise::FrameworkError<'_, Data, Error>) {
    match &error {
        poise::FrameworkError::Command { ctx, error, .. } => tracing::error!(
            command = %ctx.command().qualified_name,
            guild_id = ?ctx.guild_id(),
            %error,
            "Discord command failed"
        ),
        poise::FrameworkError::Setup { error, .. } => {
            tracing::error!(%error, "Discord framework setup failed");
        }
        poise::FrameworkError::EventHandler { error, event, .. } => tracing::error!(
            event = event.snake_case_name(),
            %error,
            "Discord event handler failed"
        ),
        poise::FrameworkError::CommandPanic { ctx, .. } => tracing::error!(
            command = %ctx.command().qualified_name,
            guild_id = ?ctx.guild_id(),
            "Discord command panicked"
        ),
        _ => tracing::error!("Discord framework rejected an interaction"),
    }
    if let Err(send_error) = poise::builtins::on_error(error).await {
        tracing::error!(?send_error, "failed to send Discord error response");
    }
}

fn spawn_database_backups(db: Arc<Database>) {
    let enabled = env::var("MUSIC_DB_BACKUP_ENABLED")
        .map(|value| !value.eq_ignore_ascii_case("false") && value != "0")
        .unwrap_or(true);
    if !enabled {
        tracing::info!("automatic SQLite backups disabled");
        return;
    }

    let backup_dir = env::var("MUSIC_DB_BACKUP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            db.path()
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("backups")
        });
    let interval_hours = env::var("MUSIC_DB_BACKUP_INTERVAL_HOURS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(24)
        .clamp(1, 24 * 365);
    let retention = env::var("MUSIC_DB_BACKUP_RETENTION")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(7)
        .clamp(1, 365);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_hours * 60 * 60));
        loop {
            interval.tick().await;
            let db = db.clone();
            let backup_dir = backup_dir.clone();
            match tokio::task::spawn_blocking(move || db.create_backup(&backup_dir, retention))
                .await
            {
                Ok(Ok(path)) => tracing::info!(path = %path.display(), "SQLite backup completed"),
                Ok(Err(error)) => tracing::error!(?error, "SQLite backup failed"),
                Err(error) => tracing::error!(?error, "SQLite backup task failed"),
            }
        }
    });
}
