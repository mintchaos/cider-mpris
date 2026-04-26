# MPRIS Bridge Completion — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the MPRIS bridge to full compliance — signal emission so desktop widgets see real-time updates, position interpolation, graceful Cider-availability handling, and eliminate all bugs and warnings.

**Architecture:** The bridge polls Cider's HTTP API every 500ms, compares state diffs, and emits `PropertiesChanged` D-Bus signals via zbus `InterfaceRef` when `PlaybackStatus` or `Metadata` change. Position is interpolated from snapshot + elapsed wall-clock time for smooth widget display.

**Tech Stack:** Rust, tokio, zbus 4, reqwest (async), serde, tracing

---

## File Structure Mapping

```
Cargo.toml            # Modified: remove reqwest blocking feature
src/
├── main.rs           # Modified: InterfaceRef capture, signal emission, availability handling
├── cider/
│   ├── mod.rs        # Modified: switch to async reqwest::Client
│   └── types.rs      # Unchanged
└── mpris/
    ├── mod.rs        # Unchanged
    ├── root.rs       # Unchanged
    └── player.rs     # Modified: position interpolation, async client bridging
```

---

## Task Decomposition

### Task 1: Switch CiderClient to async + update Player handlers

**Dependencies:** None (first task)
**Files:**
- Modify: `Cargo.toml`
- Modify: `src/cider/mod.rs`
- Modify: `src/mpris/player.rs`

**Why combined:** Changing CiderClient to async breaks the D-Bus handler callsites; both changes must be committed together to keep the code compiling.

- [ ] **Step 1: Remove `blocking` feature from reqwest in Cargo.toml**

Change the `reqwest` dependency from:
```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls", "blocking"] }
```
To:
```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
```

- [ ] **Step 2: Rewrite CiderClient to use async `reqwest::Client`**

Replace the entire contents of `src/cider/mod.rs` with:

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
            "apptoken",
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
            .await?;
        Ok(resp.json::<PlaybackStatusResponse>().await?.is_playing)
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
            Err(e) if e.is_connect() => Ok(None),
            Err(e) => Err(ClientError::Reqwest(e)),
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
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Reqwest(e)
    }
}
```

- [ ] **Step 3: Update Player D-Bus method handlers to bridge async client**

In `src/mpris/player.rs`, update all D-Bus method handlers to use `tokio::runtime::Handle::current().block_on(...)` instead of the now-removed `*_blocking()` methods.

Replace the method handler implementations (the `next`, `previous`, `pause`, `play_pause`, `stop`, `play`, `seek`, `set_position`, `open_uri` functions) with:

```rust
    fn next(&self) {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = handle.block_on(self.client.next()) {
            tracing::warn!("Next failed: {:?}", e);
        }
    }

    fn previous(&self) {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = handle.block_on(self.client.previous()) {
            tracing::warn!("Previous failed: {:?}", e);
        }
    }

    fn pause(&self) {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = handle.block_on(self.client.pause()) {
            tracing::warn!("Pause failed: {:?}", e);
        }
    }

    fn play_pause(&self) {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = handle.block_on(self.client.play_pause()) {
            tracing::warn!("PlayPause failed: {:?}", e);
        }
    }

    fn stop(&self) {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = handle.block_on(self.client.pause()) {
            tracing::warn!("Stop failed: {:?}", e);
        }
    }

    fn play(&self) {
        let handle = tokio::runtime::Handle::current();
        if let Err(e) = handle.block_on(self.client.play()) {
            tracing::warn!("Play failed: {:?}", e);
        }
    }

    fn seek(&self, offset: i64) {
        tracing::info!("Seek requested: {} us (not supported by Cider API)", offset);
    }

    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        tracing::info!("SetPosition: {} at {} us (not supported by Cider API)", track_id, position);
    }

    fn open_uri(&self, uri: &str) {
        tracing::info!("OpenUri: {} (not implemented)", uri);
    }
```

- [ ] **Step 4: Build to verify compilation**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero errors

- [ ] **Step 5: Commit**

```bash
jj describe -m "cider: switch to async reqwest client, bridge D-Bus handlers with block_on"
```

---

### Task 2: Add position interpolation

**Dependencies:** Task 1 (PlayerState structure available)
**Files:**
- Modify: `src/mpris/player.rs`

- [ ] **Step 1: Add `Instant` import and new fields to PlayerState**

In `src/mpris/player.rs`, at the top of the file, add the import:
```rust
use std::time::Instant;
```

Replace the `PlayerState` struct definition with:
```rust
pub struct PlayerState {
    pub playback_status: PlaybackStatus,
    pub now_playing: Option<NowPlayingInfo>,
    pub position_snapshot_us: i64,
    pub position_snapshot_at: Instant,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            playback_status: PlaybackStatus::Stopped,
            now_playing: None,
            position_snapshot_us: 0,
            position_snapshot_at: Instant::now(),
        }
    }
}
```

- [ ] **Step 2: Update `position` property getter to compute elapsed**

Replace the `position` property getter with:
```rust
    #[zbus(property(emits_changed_signal = "true"))]
    fn position(&self) -> i64 {
        let state = self.state.read().unwrap();
        if state.playback_status == PlaybackStatus::Playing {
            let elapsed = state.position_snapshot_at.elapsed().as_micros() as i64;
            state.position_snapshot_us + elapsed
        } else {
            state.position_snapshot_us
        }
    }
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero errors

- [ ] **Step 4: Commit**

```bash
jj describe -m "player: add position interpolation from wall-clock elapsed"
```

---

### Task 3: Rewrite polling loop — signals, availability, cleanup

**Dependencies:** Tasks 1 and 2 (async client + interpolation available)
**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace entire main.rs**

Replace the entire contents of `src/main.rs` with:

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zbus::Connection;
use zbus::zvariant::Value;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cider;
mod mpris;

use crate::cider::CiderClient;
use crate::mpris::root::Root;
use crate::mpris::player::{Player, PlayerState, PlaybackStatus};

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

    let cider_client = Arc::new(CiderClient::new(token));
    let player_state = Arc::new(RwLock::new(PlayerState::default()));

    // D-Bus setup
    let conn = Connection::session().await?;
    
    // Request the well-known name
    conn.request_name("org.mpris.MediaPlayer2.cider").await?;
    
    let object_server = conn.object_server();
    
    // Register Root interface
    let _root_iface = object_server
        .at("/org/mpris/MediaPlayer2", Root::new())
        .await?;
    
    // Register Player interface and capture InterfaceRef for signal emission
    let player = Player::new(
        Arc::clone(&cider_client),
        Arc::clone(&player_state),
    );
    let player_iface = object_server
        .at("/org/mpris/MediaPlayer2", player)
        .await?;

    // Polling task with property change detection and signal emission
    let state_for_poll = Arc::clone(&player_state);
    let client_for_poll = Arc::clone(&cider_client);
    let player_iface_for_poll = player_iface.clone();

    tokio::spawn(async move {
        let active_interval = tokio::time::Duration::from_millis(500);
        let retry_interval = tokio::time::Duration::from_secs(2);
        let mut cider_available = true;
        let mut prev_status: Option<PlaybackStatus> = None;
        let mut prev_metadata_key: Option<String> = None;
        
        loop {
            // --- Check playback status from Cider ---
            let is_playing = match client_for_poll.is_playing().await {
                Ok(playing) => playing,
                Err(e) => {
                    // Cider became unavailable
                    if cider_available {
                        tracing::warn!("Cider became unavailable: {:?}", e);
                        cider_available = false;
                        
                        let mut s = state_for_poll.write().unwrap();
                        if s.playback_status != PlaybackStatus::Stopped
                            || s.now_playing.is_some()
                        {
                            s.playback_status = PlaybackStatus::Stopped;
                            s.now_playing = None;
                            drop(s);
                            
                            let _ = player_iface_for_poll
                                .playback_status_changed("Stopped")
                                .await;
                            let _ = player_iface_for_poll
                                .metadata_changed(HashMap::new())
                                .await;
                        } else {
                            drop(s);
                        }
                        prev_status = Some(PlaybackStatus::Stopped);
                        prev_metadata_key = None;
                    }
                    tokio::time::sleep(retry_interval).await;
                    continue;
                }
            };
            
            // Cider responded — mark available and restore active polling
            if !cider_available {
                tracing::info!("Cider is now available");
                cider_available = true;
            }
            
            let new_status = if is_playing {
                PlaybackStatus::Playing
            } else {
                PlaybackStatus::Paused
            };
            
            // --- Fetch now-playing metadata ---
            match client_for_poll.now_playing().await {
                Ok(Some(info)) => {
                    let mut s = state_for_poll.write().unwrap();
                    
                    let meta_key = format!("{}|{}|{}", info.name, info.artist_name, info.album_name);
                    let metadata_changed = prev_metadata_key.as_deref() != Some(&meta_key);
                    let status_changed = Some(&new_status) != prev_status.as_ref();
                    
                    // Update state
                    s.playback_status = new_status.clone();
                    s.now_playing = Some(info);
                    let current_pt = s.now_playing.as_ref().unwrap().current_playback_time;
                    s.position_snapshot_us = (current_pt * 1_000_000.0) as i64;
                    s.position_snapshot_at = std::time::Instant::now();
                    
                    // Build metadata snapshot (while lock held)
                    let metadata_for_signal = build_metadata_map(&s);
                    
                    drop(s);
                    
                    // Emit signals outside the lock
                    if status_changed {
                        let status_str = new_status.as_str();
                        let _ = player_iface_for_poll
                            .playback_status_changed(status_str)
                            .await;
                        prev_status = Some(new_status);
                    }
                    
                    if metadata_changed {
                        let _ = player_iface_for_poll
                            .metadata_changed(metadata_for_signal)
                            .await;
                        prev_metadata_key = Some(meta_key);
                    }
                }
                Ok(None) => {
                    // Cider reports nothing playing — treat as Stopped
                    let mut s = state_for_poll.write().unwrap();
                    if s.playback_status != PlaybackStatus::Stopped
                        || s.now_playing.is_some()
                    {
                        s.playback_status = PlaybackStatus::Stopped;
                        s.now_playing = None;
                        drop(s);
                        
                        let _ = player_iface_for_poll
                            .playback_status_changed("Stopped")
                            .await;
                        let _ = player_iface_for_poll
                            .metadata_changed(HashMap::new())
                            .await;
                        prev_status = Some(PlaybackStatus::Stopped);
                        prev_metadata_key = None;
                    } else {
                        drop(s);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch now playing: {:?}", e);
                }
            }
            
            tokio::time::sleep(active_interval).await;
        }
    });

    tracing::info!("cider-mpris bridge running on D-Bus as org.mpris.MediaPlayer2.cider");
    tracing::info!("Press Ctrl+C to stop.");
    
    // Keep alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
    }
}

/// Build an MPRIS-compatible metadata HashMap from the current player state.
fn build_metadata_map(state: &PlayerState) -> HashMap<String, Value<'static>> {
    let mut map = HashMap::new();
    
    if let Some(info) = &state.now_playing {
        map.insert(
            "mpris:trackid".to_string(),
            Value::new("/org/mpris/MediaPlayer2/TrackList/NoTrack"),
        );
        map.insert(
            "mpris:length".to_string(),
            Value::new((info.duration_in_millis as i64) * 1000),
        );
        map.insert(
            "xesam:artist".to_string(),
            Value::new(vec![info.artist_name.clone()]),
        );
        map.insert(
            "xesam:title".to_string(),
            Value::new(info.name.clone()),
        );
        map.insert(
            "xesam:album".to_string(),
            Value::new(info.album_name.clone()),
        );
        
        if let Some(artwork) = &info.artwork {
            map.insert(
                "mpris:artUrl".to_string(),
                Value::new(artwork.url.clone()),
            );
        }
    }
    
    map
}
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero errors (there may be warnings about unused `CiderClient` methods — fixed in Task 4)

- [ ] **Step 3: Commit**

```bash
jj describe -m "main: rewrite polling loop with signal emission and availability handling"
```

---

### Task 4: Clean up remaining warnings

**Dependencies:** Task 3 (compiling codebase)
**Files:**
- Modify: `src/cider/types.rs`
- Modify: `src/cider/mod.rs`

- [ ] **Step 1: Remove unused `ClientError` warning**

The `ClientError::Reqwest` field is never read directly (only used via `Debug`/`Display`). Suppress the lint by adding `#[allow(dead_code)]` to the field, or use it in an error display impl. We'll take the simplest fix — suppress the warning.

In `src/cider/mod.rs`, change the `ClientError` enum to:
```rust
#[derive(Debug)]
pub enum ClientError {
    #[allow(dead_code)]
    Reqwest(reqwest::Error),
}
```

- [ ] **Step 2: Remove unused `Serialize` derive on types if not needed**

Check `src/cider/types.rs`. The `EmptyRequest` type mentioned in the plan doc doesn't exist in the actual code. No changes needed here — the types are all `Deserialize`-only and used.

- [ ] **Step 3: Handle unused CiderClient async methods (if any remain)**

After Task 3, all CiderClient methods are used (polling loop uses `is_playing` + `now_playing`, D-Bus handlers use `play`/`pause`/`play_pause`/`next`/`previous`). No dead code warnings expected for CiderClient methods.

- [ ] **Step 4: Build and verify zero warnings**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero warnings and zero errors

- [ ] **Step 5: Commit**

```bash
jj describe -m "cleanup: suppress dead_code warning on ClientError field"
```

---

### Task 5 (Final): End-to-End Verification

**Dependencies:** All preceding tasks
**Files:** None (read-only verification)

- [ ] **Step 1: Clean build**

Run: `cargo build --release 2>&1`
Expected: SUCCESS, zero warnings, zero errors

- [ ] **Step 2: Start bridge (requires Cider running)**

Run in background:
```bash
./target/release/cider-mpris &
```

Expected: `"Starting cider-mpris bridge"` and `"cider-mpris bridge running on D-Bus..."` in logs

- [ ] **Step 3: Verify D-Bus registration**

Run:
```bash
dbus-send --session --dest=org.freedesktop.DBus --type=method_call --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames | grep cider
```

Expected: `org.mpris.MediaPlayer2.cider` appears

- [ ] **Step 4: Verify playback status reflects real Cider state**

Run:
```bash
playerctl -p cider status
```

Expected: `Playing` (if Cider is playing) or `Paused` (if paused)

- [ ] **Step 5: Verify metadata visible**

Run:
```bash
playerctl -p cider metadata
```

Expected: Shows title, artist, album, artUrl, length, trackid

- [ ] **Step 6: Change track in Cider and verify metadata updates**

Switch to different track/album in Cider, wait up to 1s, then run:
```bash
playerctl -p cider metadata
```

Expected: Metadata reflects the new track

- [ ] **Step 7: Test play-pause via playerctl**

Run:
```bash
playerctl -p cider play-pause
playerctl -p cider status
```

Expected: Status toggles between `Playing` and `Paused`

- [ ] **Step 8: Verify DMS widget bar updates**

Check the DMS widget bar — it should:
- Show current track/artist/album art
- Update when track changes in Cider
- Show paused state and allow resuming via play button
- Track position should advance smoothly

- [ ] **Step 9: Test Cider-unavailable behavior**

Stop Cider completely. Then run:
```bash
playerctl -p cider status
```

Expected: `Stopped` (bridge stays alive, no repeated error spam)

- [ ] **Step 10: Test Cider recovery**

Restart Cider and start playing. Run:
```bash
playerctl -p cider status
playerctl -p cider metadata
```

Expected: Within a few seconds, status returns to `Playing`/`Paused` with full metadata

- [ ] **Step 11: Test clean shutdown**

```bash
kill %1   # or Ctrl+C if running in foreground
```

Expected: No panic, no "Cannot drop a runtime in a context where blocking is not allowed" error

- [ ] **Step 12: Commit final verification results**

No files changed — just confirm all checks pass.

---

## Success Criteria

1. **Zero warnings** — `cargo build` produces no warnings
2. **Signal emission** — Widgets (DMS) reflect track changes, play/pause state within 1-2 poll cycles
3. **Position interpolation** — DMS progress bar advances smoothly between polls
4. **Cider availability** — Bridge reports `Stopped` gracefully when Cider is down; recovers when Cider restarts
5. **Clean shutdown** — No runtime drop panic on exit
6. **playerctl integration** — All standard `playerctl` commands work: `status`, `metadata`, `play-pause`, `next`, `previous`
