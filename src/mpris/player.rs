use std::collections::HashMap;
use std::time::Instant;
use std::sync::{Arc, RwLock};
use zbus::interface;
use zbus::zvariant::{ObjectPath, Value};
use crate::cider::types::NowPlayingInfo;
use crate::cider::CiderClient;

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackStatus::Playing => "Playing",
            PlaybackStatus::Paused => "Paused",
            PlaybackStatus::Stopped => "Stopped",
        }
    }
}

pub struct PlayerState {
    pub playback_status: PlaybackStatus,
    pub now_playing: Option<NowPlayingInfo>,
    pub position_snapshot_us: i64,
    pub position_snapshot_at: Instant,
    pub repeat_mode: u8,
    pub shuffle_mode: u8,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            playback_status: PlaybackStatus::Stopped,
            now_playing: None,
            position_snapshot_us: 0,
            position_snapshot_at: Instant::now(),
            repeat_mode: 0,
            shuffle_mode: 0,
        }
    }
}

pub struct Player {
    pub client: Arc<CiderClient>,
    pub state: Arc<RwLock<PlayerState>>,
}

impl Player {
    pub fn new(
        client: Arc<CiderClient>, 
        state: Arc<RwLock<PlayerState>>,
    ) -> Self {
        Self { client, state }
    }
}

#[interface(interface = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn next(&self) {
        if let Err(e) = self.client.next().await {
            tracing::warn!("Next failed: {:?}", e);
        }
    }

    async fn previous(&self) {
        if let Err(e) = self.client.previous().await {
            tracing::warn!("Previous failed: {:?}", e);
        }
    }

    async fn pause(&self) {
        if let Err(e) = self.client.pause().await {
            tracing::warn!("Pause failed: {:?}", e);
        }
    }

    async fn play_pause(&self) {
        if let Err(e) = self.client.play_pause().await {
            tracing::warn!("PlayPause failed: {:?}", e);
        }
    }

    async fn stop(&self) {
        if let Err(e) = self.client.pause().await {
            tracing::warn!("Stop failed: {:?}", e);
        }
    }

    async fn play(&self) {
        if let Err(e) = self.client.play().await {
            tracing::warn!("Play failed: {:?}", e);
        }
    }

    async fn seek(&self, offset: i64) {
        let current_pos = {
            let state = self.state.read().unwrap();
            if state.playback_status == PlaybackStatus::Playing {
                let elapsed = state.position_snapshot_at.elapsed().as_micros() as i64;
                state.position_snapshot_us.saturating_add(elapsed)
            } else {
                state.position_snapshot_us
            }
        };
        let target_us = (current_pos + offset).max(0);
        let target_seconds = target_us as f64 / 1_000_000.0;
        if let Err(e) = self.client.seek(target_seconds).await {
            tracing::warn!("Seek failed: {:?}", e);
        }
    }

    async fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) {
        let target_seconds = (position.max(0)) as f64 / 1_000_000.0;
        if let Err(e) = self.client.seek(target_seconds).await {
            tracing::warn!("SetPosition failed: {:?}", e);
        }
    }

    fn open_uri(&self, uri: &str) {
        tracing::info!("OpenUri: {} (not implemented)", uri);
    }

    #[zbus(property(emits_changed_signal = "true"))]
    fn playback_status(&self) -> String {
        self.state.read()
            .map(|s| s.playback_status.as_str().to_string())
            .unwrap_or_else(|_| "Stopped".to_string())
    }

    #[zbus(property(emits_changed_signal = "true"))]
    fn loop_status(&self) -> String {
        match self.state.read().unwrap().repeat_mode {
            1 => "Track".to_string(),
            2 => "Playlist".to_string(),
            _ => "None".to_string(),
        }
    }

    #[zbus(property)]
    async fn set_loop_status(&self, _value: &str) {
        if let Err(e) = self.client.toggle_repeat().await {
            tracing::warn!("Toggle repeat failed: {:?}", e);
        }
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "true"))]
    fn shuffle(&self) -> bool {
        self.state.read().unwrap().shuffle_mode != 0
    }

    #[zbus(property)]
    async fn set_shuffle(&self, _value: bool) {
        if let Err(e) = self.client.toggle_shuffle().await {
            tracing::warn!("Toggle shuffle failed: {:?}", e);
        }
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "true"))]
    fn position(&self) -> i64 {
        let state = self.state.read().unwrap();
        if state.playback_status == PlaybackStatus::Playing {
            let elapsed = state.position_snapshot_at.elapsed().as_micros() as i64;
            state.position_snapshot_us.saturating_add(elapsed)
        } else {
            state.position_snapshot_us
        }
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(property(emits_changed_signal = "true"))]
    fn metadata(&self) -> HashMap<String, Value<'_>> {
        let mut map = HashMap::new();
        
        if let Ok(state) = self.state.read() {
            if let Some(info) = &state.now_playing {
                map.insert("mpris:trackid".to_string(), Value::new("/org/mpris/MediaPlayer2/TrackList/NoTrack"));
                map.insert("mpris:length".to_string(), Value::new((info.duration_in_millis as i64) * 1000));
                map.insert("xesam:artist".to_string(), Value::new(vec![info.artist_name.clone()]));
                map.insert("xesam:title".to_string(), Value::new(info.name.clone()));
                map.insert("xesam:album".to_string(), Value::new(info.album_name.clone()));
                
                // Add artwork URL if available
                if let Some(artwork) = &info.artwork {
                    map.insert("mpris:artUrl".to_string(), Value::new(artwork.url.clone()));
                }
            }
        }
        
        map
    }
}