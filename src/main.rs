mod commands;
mod interactions;
mod music;
mod storage;
mod ui;

use std::{env, sync::Arc};

use poise::serenity_prelude as serenity;
use serenity::{ClientBuilder, GatewayIntents};
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

    let token = env::var("DISCORD_TOKEN")
        .expect("DISCORD_TOKEN belum diisi. Copy .env.example ke .env lalu isi token bot.");
    let db_path = env::var("MUSIC_DB_PATH").unwrap_or_else(|_| "music_bot.db".to_string());
    let db = Arc::new(Database::new(db_path)?);
    tracing::info!("Using music database at {}", db.path().display());

    let intents = GatewayIntents::non_privileged() | GatewayIntents::GUILD_VOICE_STATES;

    let options = poise::FrameworkOptions {
        commands: vec![
            commands::play::play(),
            commands::queue::queue(),
            commands::now::now(),
            commands::leave::leave(),
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
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .options(options)
        .setup(|ctx, _ready, framework| {
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

                Ok(Data {
                    music: Arc::new(MusicStore::default()),
                    http_client: reqwest::Client::new(),
                    db,
                })
            })
        })
        .build();

    let mut client = ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird()
        .await?;

    client.start().await?;

    Ok(())
}
