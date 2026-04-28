use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct NowPlayingResponse {
    pub info: NowPlayingInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NowPlayingInfo {
    pub name: String,
    #[serde(rename = "artistName")]
    pub artist_name: String,
    #[serde(rename = "albumName")]
    pub album_name: String,
    #[serde(rename = "durationInMillis")]
    pub duration_in_millis: u64,
    #[serde(rename = "currentPlaybackTime")]
    pub current_playback_time: f64,
    pub artwork: Option<Artwork>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artwork {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaybackStatusResponse {
    pub is_playing: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepeatModeResponse {
    #[serde(rename = "repeatMode")]
    pub repeat_mode: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShuffleModeResponse {
    #[serde(rename = "shuffleMode")]
    pub shuffle_mode: u8,
}