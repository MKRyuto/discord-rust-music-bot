use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;
use tokio::sync::Mutex;

use super::track::Track;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    Off,
    One,
    Queue,
}

impl LoopMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::Queue,
            Self::Queue => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::One => "One",
            Self::Queue => "Queue",
        }
    }
}

#[derive(Debug)]
pub struct GuildMusicState {
    pub queue: VecDeque<Track>,
    pub now_playing: Option<Track>,
    pub current_handle: Option<TrackHandle>,
    pub suppress_next_end: bool,
    pub is_paused: bool,
    pub loop_mode: LoopMode,
    pub player_message_id: Option<serenity::MessageId>,
    pub player_channel_id: Option<serenity::ChannelId>,
    pub queue_page: usize,
}

impl Default for GuildMusicState {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            now_playing: None,
            current_handle: None,
            suppress_next_end: false,
            is_paused: false,
            loop_mode: LoopMode::Off,
            player_message_id: None,
            player_channel_id: None,
            queue_page: 0,
        }
    }
}

#[derive(Default)]
pub struct MusicStore {
    inner: Mutex<HashMap<serenity::GuildId, Arc<Mutex<GuildMusicState>>>>,
}

impl MusicStore {
    pub async fn get(&self, guild_id: serenity::GuildId) -> Arc<Mutex<GuildMusicState>> {
        let mut map = self.inner.lock().await;

        map.entry(guild_id)
            .or_insert_with(|| Arc::new(Mutex::new(GuildMusicState::default())))
            .clone()
    }
}
