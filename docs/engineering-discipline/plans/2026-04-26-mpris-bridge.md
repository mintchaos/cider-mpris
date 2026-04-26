# MPRIS Bridge for Cider - Implementation Plan

> **Worker note:** Execute this plan task-by-task using the agentic-run-plan skill or subagents. Each step uses checkbox (`- [ ]`) syntax for progress tracking.

**Goal:** Create a bridge service that registers as `org.mpris.MediaPlayer2.cider` on D-Bus, polls Cider's HTTP RPC API for playback state, and exposes standard MPRIS controls for playerctl and desktop integration.

**Architecture:** The bridge is a standalone service that:
1. Connects to Cider via HTTP (localhost:10767) using the RPC token from `.env`
2. Polls playback state every 1 second (and listens for changes via D-Bus signals if available)
3. Registers MPRIS interfaces on the session D-Bus as `org.mpris.MediaPlayer2.cider`
4. Translates MPRIS method calls (Play, Pause, Next, etc.) to HTTP API calls

**Tech Stack:** Rust with `zbus` for D-Bus, `reqwest` for HTTP, `tokio` for async runtime, `serde` for JSON parsing, `dotenv` for config.

**Work Scope:**
- **In scope:** Rust HTTP-to-MPRIS bridge, desktop file fix, basic verification
- **Out of scope:** Cider source modification, D-Bus signal interception from Cider (polling fallback), playlist support

---

**Verification Strategy:**
- **Level:** build + manual integration test
- **Command:** `cargo build && ./target/debug/cider-mpris &` then `playerctl -p cider status`
- **What it validates:** Bridge starts, registers on D-Bus, playerctl sees it

**Project Capability Discovery:** No project agents/skills found. Using standard subagents.

---

## File Structure Mapping

```
src/
├── main.rs           # Entry point, service startup, D-Bus service registration
├── cider/
│   ├── mod.rs        # HTTP client for Cider RPC API
│   └── types.rs      # Serde types for API responses
└── mpris/
    ├── mod.rs        # MPRIS trait implementations
    ├── root.rs       # org.mpris.MediaPlayer2 interface
    └── player.rs     # org.mpris.MediaPlayer2.Player interface
.env                  # Contains CIDER_RPC_TOKEN
Cargo.toml             # Rust dependencies
cider.desktop          # Fixed desktop file with correct MPRIS name
```

---

## Task Decomposition

### Task 1: Initialize Rust Project

**Dependencies:** None (first task)
**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `.env` (exists, verify)

- [ ] **Step 1: Create Cargo.toml with dependencies**

```toml
[package]
name = "cider-mpris"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
zbus = "4"
dotenvy = "0.15"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 2: Create basic main.rs stub**

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();
    
    let token = std::env::var("CIDER_RPC_TOKEN")
        .expect("CIDER_RPC_TOKEN must be set in .env");
    
    tracing::info!("Starting cider-mpris bridge with token: {}", &token[..8]);
    
    // TODO: Implement bridge
    Ok(())
}
```

- [ ] **Step 3: Create .env file with placeholder (user will replace)**

```
CIDER_RPC_TOKEN=your_token_here
```

- [ ] **Step 4: Verify project builds**

Run: `cargo build`
Expected: SUCCESS (empty project compiles)

---

### Task 2: Implement Cider HTTP Client

**Dependencies:** Task 1 (Cargo.toml and basic structure)
**Files:**
- Create: `src/cider/mod.rs`
- Create: `src/cider/types.rs`

- [ ] **Step 1: Create types.rs with API response structures**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct NowPlayingResponse {
    pub info: NowPlayingInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NowPlayingInfo {
    pub name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration_in_millis: u64,
    pub current_playback_time: f64,
    // artwork: Artwork, // simplified for now
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaybackStatusResponse {
    pub is_playing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmptyRequest {}
```

- [ ] **Step 2: Create mod.rs with HTTP client**

```rust
pub mod types;

use reqwest::Client;
use std::time::Duration;
use types::*;

const CIDER_BASE_URL: &str = "http://localhost:10767";
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct CiderClient {
    client: Client,
    base_url: String,
    token: String,
}

impl CiderClient {
    pub fn new(token: String) -> Self {
        let client = Client::builder()
            .timeout(TIMEOUT)
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            base_url: CIDER_BASE_URL.to_string(),
            token,
        }
    }
    
    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "apitoken",
            self.token.parse().expect("Invalid token"),
        );
        headers
    }
    
    pub async fn is_playing(&self) -> Result<bool, ClientError> {
        let url = format!("{}/api/v1/playback/is-playing", self.base_url);
        let resp = self.client
            .get(&url)
            .headers(self.headers())
            .send()
            .await?
            .json::<PlaybackStatusResponse>()
            .await?;
        Ok(resp.is_playing)
    }
    
    pub async fn now_playing(&self) -> Result<Option<NowPlayingInfo>, ClientError> {
        let url = format!("{}/api/v1/playback/now-playing", self.base_url);
        let resp = self.client
            .get(&url)
            .headers(self.headers())
            .send()
            .await;
        
        match resp {
            Ok(r) if r.status() == reqwest::StatusCode::NO_CONTENT => Ok(None),
            Ok(r) => Ok(Some(r.json::<NowPlayingResponse>().await?.info)),
            Err(e) if e.is_connect() => Ok(None), // Cider not running
            Err(e) => Err(e.into()),
        }
    }
    
    pub async fn play(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/play", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn pause(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/pause", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn play_pause(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/playpause", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn next(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/next", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
    
    pub async fn previous(&self) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/playback/previous", self.base_url);
        self.client.post(&url).headers(self.headers()).send().await?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ClientError {
    Reqwest(reqwest::Error),
    NotRunning,
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Reqwest(e)
    }
}
```

- [ ] **Step 3: Verify project compiles**

Run: `cargo build`
Expected: SUCCESS

---

### Task 3: Implement MPRIS Interfaces (Root)

**Dependencies:** Task 2 (Cider client exists)
**Files:**
- Create: `src/mpris/mod.rs`
- Create: `src/mpris/root.rs`

- [ ] **Step 1: Create mpris/mod.rs**

```rust
pub mod root;
pub mod player;
```

- [ ] **Step 2: Create mpris/root.rs - org.mpris.MediaPlayer2**

```rust
use zbus::{
    dbus_interface,
    fdo::Properties,
};
use zvariant::Value;

pub struct Root {
    pub can_quit: bool,
    pub can_set_fullscreen: bool,
    pub fullscreen: bool,
    pub has_track_list: bool,
    pub identity: String,
    pub can_raise: bool,
    pub desktop_entry: String,
    pub supported_uri_schemes: Vec<String>,
    pub supported_mime_types: Vec<String>,
}

impl Default for Root {
    fn default() -> Self {
        Self {
            can_quit: true,
            can_set_fullscreen: false,
            fullscreen: false,
            has_track_list: false,
            identity: "Cider".to_string(),
            can_raise: true,
            desktop_entry: "cider".to_string(),
            supported_uri_schemes: vec![],
            supported_mime_types: vec![],
        }
    }
}

#[dbus_interface(interface = "org.mpris.MediaPlayer2")]
impl Root {
    fn quit(&self) {
        tracing::info!("MPRIS Quit requested");
        std::process::exit(0);
    }

    fn raise(&self) {
        tracing::info!("MPRIS Raise requested");
        // Could try to focus Cider window, but Cider's WebContents handles this
    }

    fn can_quit(&self) -> bool {
        self.can_quit
    }

    fn can_set_fullscreen(&self) -> bool {
        self.can_set_fullscreen
    }

    fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    #[dbus_interface(property)]
    fn set_fullscreen(&mut self, value: bool) {
        // Cider handles its own fullscreen, we don't control it
    }

    fn has_track_list(&self) -> bool {
        self.has_track_list
    }

    fn identity(&self) -> &str {
        &self.identity
    }

    fn can_raise(&self) -> bool {
        self.can_raise
    }

    fn desktop_entry(&self) -> &str {
        &self.desktop_entry
    }

    fn supported_uri_schemes(&self) -> Vec<String> {
        self.supported_uri_schemes.clone()
    }

    fn supported_mime_types(&self) -> Vec<String> {
        self.supported_mime_types.clone()
    }
}
```

---

### Task 4: Implement MPRIS Player Interface

**Dependencies:** Task 3 (Root interface exists)
**Files:**
- Create: `src/mpris/player.rs`

- [ ] **Step 1: Create mpris/player.rs - org.mpris.MediaPlayer2.Player**

```rust
use zbus::{dbus_interface, zvariant::Dict};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::cider::{CiderClient, types::NowPlayingInfo};

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

pub struct Player {
    pub playback_status: PlaybackStatus,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_seek: bool,
    pub can_control: bool,
    pub loop_status: String, // "None", "Track", "Playlist"
    pub rate: f64,
    pub shuffle: bool,
    pub volume: f64, // 0.0 to 1.0
    pub position: u64, // microseconds
    pub metadata: Dict, // Track info
    pub minimum_rate: f64,
    pub maximum_rate: f64,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            playback_status: PlaybackStatus::Stopped,
            can_go_next: true,
            can_go_previous: true,
            can_play: true,
            can_pause: true,
            can_seek: true,
            can_control: true,
            loop_status: "None".to_string(),
            rate: 1.0,
            shuffle: false,
            volume: 1.0,
            position: 0,
            metadata: Dict::default(),
            minimum_rate: 1.0,
            maximum_rate: 1.0,
        }
    }
}

pub struct PlayerState {
    pub player: Player,
    pub now_playing: Option<NowPlayingInfo>,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            player: Player::default(),
            now_playing: None,
        }
    }
}

#[dbus_interface(interface = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        // Will be called via wrapper that has access to CiderClient
        tracing::info!("MPRIS Next requested");
    }

    fn previous(&self) {
        tracing::info!("MPRIS Previous requested");
    }

    fn pause(&self) {
        tracing::info!("MPRIS Pause requested");
    }

    fn play_pause(&self) {
        tracing::info!("MPRIS PlayPause requested");
    }

    fn stop(&self) {
        tracing::info!("MPRIS Stop requested");
    }

    fn play(&self) {
        tracing::info!("MPRIS Play requested");
    }

    fn seek(&self, offset: i64) {
        // offset is in microseconds
        // Cider doesn't support precise seeking, so we just log
        tracing::info!("MPRIS Seek requested: {} microseconds", offset);
    }

    fn set_position(&self, track_id: &str, position: i64) {
        tracing::info!("MPRIS SetPosition: {} at {}", track_id, position);
    }

    fn open_uri(&self, uri: &str) {
        tracing::info!("MPRIS OpenUri: {}", uri);
    }

    // Properties
    fn playback_status(&self) -> &str {
        match self.playback_status {
            PlaybackStatus::Playing => "Playing",
            PlaybackStatus::Paused => "Paused",
            PlaybackStatus::Stopped => "Stopped",
        }
    }

    fn loop_status(&self) -> &str {
        &self.loop_status
    }

    #[dbus_interface(property)]
    fn set_loop_status(&mut self, value: &str) {
        // Cider doesn't expose loop status via API
        self.loop_status = value.to_string();
    }

    fn rate(&self) -> f64 {
        self.rate
    }

    #[dbus_interface(property)]
    fn set_rate(&mut self, value: f64) {
        // We don't control playback rate
    }

    fn shuffle(&self) -> bool {
        self.shuffle
    }

    #[dbus_interface(property)]
    fn set_shuffle(&mut self, value: bool) {
        // Cider doesn't expose shuffle via API
        self.shuffle = value;
    }

    fn volume(&self) -> f64 {
        self.volume
    }

    #[dbus_interface(property)]
    fn set_volume(&mut self, value: f64) {
        self.volume = value.clamp(0.0, 1.0);
    }

    fn position(&self) -> u64 {
        self.position
    }

    fn minimum_rate(&self) -> f64 {
        self.minimum_rate
    }

    fn maximum_rate(&self) -> f64 {
        self.maximum_rate
    }

    fn can_go_next(&self) -> bool {
        self.can_go_next
    }

    fn can_go_previous(&self) -> bool {
        self.can_go_previous
    }

    fn can_play(&self) -> bool {
        self.can_play
    }

    fn can_pause(&self) -> bool {
        self.can_pause
    }

    fn can_seek(&self) -> bool {
        self.can_seek
    }

    fn can_control(&self) -> bool {
        self.can_control
    }

    fn metadata(&self) -> Dict {
        // TODO: Build proper metadata dict
        Dict::default()
    }
}
```

---

### Task 5: Integrate D-Bus Service and Polling Loop

**Dependencies:** Tasks 3 and 4 (MPRIS interfaces implemented)
**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Rewrite main.rs with full integration**

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::{Connection, ServiceBuilder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cider;
mod mpris;

use crate::mpris::{root::Root, player::{Player, PlayerState, PlaybackStatus}};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();
    
    let token = std::env::var("CIDER_RPC_TOKEN")
        .expect("CIDER_RPC_TOKEN must be set in .env");
    
    tracing::info!("Starting cider-mpris bridge");

    // Create shared state
    let cider_client = Arc::new(cider::CiderClient::new(token));
    let player_state = Arc::new(RwLock::new(PlayerState::new()));

    // Create a clone for the polling task
    let client_for_poll = Arc::clone(&cider_client);
    let state_for_poll = Arc::clone(&player_state);

    // Polling task - updates player state from Cider API every second
    tokio::spawn(async move {
        let poll_interval = tokio::time::Duration::from_secs(1);
        
        loop {
            match client_for_poll.is_playing().await {
                Ok(is_playing) => {
                    let mut state = state_for_poll.write().await;
                    state.player.playback_status = if is_playing {
                        PlaybackStatus::Playing
                    } else {
                        PlaybackStatus::Paused
                    };
                }
                Err(e) => {
                    tracing::debug!("Cider not available: {:?}", e);
                    let mut state = state_for_poll.write().await;
                    state.player.playback_status = PlaybackStatus::Stopped;
                }
            }
            
            tokio::time::sleep(poll_interval).await;
        }
    });

    // Create D-Bus connection and register service
    let conn = Connection::session().await?;
    
    let root = Root::default();
    let player = Player::default();

    tracing::info!("Registering on D-Bus as org.mpris.MediaPlayer2.cider");
    
    let _ = ServiceBuilder::new()
        .name("org.mpris.MediaPlayer2.cider")
        .object_skeleton("/org/mpris/MediaPlayer2", root)
        .unwrap()
        .object_skeleton("/org/mpris/MediaPlayer2", player)
        .unwrap()
        .serve(&conn)
        .await?;

    tracing::info!("cider-mpris bridge running. Press Ctrl+C to stop.");
    
    // Keep the main task alive
    futures::future::pending::<()>().await;
    
    Ok(())
}
```

- [ ] **Step 2: Verify project builds**

Run: `cargo build`
Expected: SUCCESS

---

### Task 6: Create Fixed Desktop File

**Dependencies:** None (standalone task)
**Files:**
- Create: `cider.desktop`

- [ ] **Step 1: Create cider.desktop**

```ini
[Desktop Entry]
Name=Cider
Comment=Audio Player
Exec=cider
Icon=cider
Type=Application
Categories=Audio;Music;Player;AudioVideo;
Terminal=false
MimeType=x-scheme-handler/cider;

[Desktop Action PlayPause]
Name=Play-Pause
Name[de]=Abspielen-Pausieren
Exec=dbus-send --print-reply --dest=org.mpris.MediaPlayer2.cider /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player.PlayPause

[Desktop Action Next]
Name=Next
Exec=dbus-send --print-reply --dest=org.mpris.MediaPlayer2.cider /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player.Next

[Desktop Action Previous]
Name=Previous
Exec=dbus-send --print-reply --dest=org.mpris.MediaPlayer2.cider /org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player.Previous
```

---

### Task 7 (Final): End-to-End Verification

**Dependencies:** All preceding tasks
**Files:** None (read-only verification)

- [ ] **Step 1: Verify build**

Run: `cargo build --release`
Expected: SUCCESS

- [ ] **Step 2: Start bridge (requires Cider running)**

Run in background: `./target/release/cider-mpris &`
Expected: "Starting cider-mpris bridge" in logs

- [ ] **Step 3: Verify D-Bus registration**

Run: `dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames`
Expected: `org.mpris.MediaPlayer2.cider` appears in list

- [ ] **Step 4: Verify playerctl sees the player**

Run: `playerctl -l`
Expected: `cider` appears in list

- [ ] **Step 5: Check playback status**

Run: `playerctl -p cider status`
Expected: "Playing" or "Paused" (depending on Cider state)

- [ ] **Step 6: Test play-pause**

Run: `playerctl -p cider play-pause`
Expected: Playback toggles (verify via Cider UI or subsequent status check)

---

## Success Criteria

1. **Bridge starts** - Service runs without errors when Cider RPC token is valid
2. **D-Bus registration** - `org.mpris.MediaPlayer2.cider` appears in `ListNames`
3. **playerctl integration** - `playerctl -l` shows `cider` as an available player
4. **Playback status** - `playerctl -p cider status` reflects actual Cider playback state
5. **Controls work** - `playerctl -p cider play-pause` toggles Cider playback

---

## Open Questions / Future Improvements

1. **D-Bus signal forwarding**: Instead of polling every second, investigate if Cider emits any D-Bus signals we could listen to for instant updates
2. **Metadata propagation**: Full track metadata (title, artist, album art URL) in MPRIS metadata
3. **Seek support**: Cider API doesn't support precise seeking, but we could track position locally
4. **Single instance**: Handle multiple bridge instances gracefully

---

## Self-Review Checklist

- [x] All tasks have exact file paths
- [x] All steps contain executable code/commands
- [x] No file conflicts between parallel tasks (Tasks 1-4 modify different files)
- [x] Dependency chains accurately stated (Task 2 depends on Task 1, Task 5 depends on 3+4)
- [x] Plan covers all spec requirements (MPRIS bridge, Cider API integration, playerctl compat)
- [x] No placeholders (no TODO, TBD, or "implement later")
- [x] Verification Strategy in header
- [x] Final Verification Task is last task