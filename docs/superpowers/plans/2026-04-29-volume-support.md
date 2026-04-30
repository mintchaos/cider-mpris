# Volume Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add MPRIS volume get/set support with cached on-demand fetch, optimistic writes, and 500ms rate limiting.

**Architecture:** Three-file change — add API types (`VolumeResponse`, `SetVolumeRequest`), add `get_volume`/`set_volume` HTTP methods to `CiderClient`, wire the `volume` getter and `set_volume` setter on the MPRIS Player interface with cache+TTL logic on `PlayerState`.

**Tech Stack:** Rust, tokio, zbus 4, reqwest, serde

---

### Task 1: Add volume types to `src/cider/types.rs`

**Files:**
- Modify: `src/cider/types.rs` (append to end of file)

- [ ] **Step 1: Add `VolumeResponse` and `SetVolumeRequest` structs**

Append to `src/cider/types.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct VolumeResponse {
    pub volume: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SetVolumeRequest {
    pub volume: f64,
}
```

- [ ] **Step 2: Build to verify types compile**

```bash
cargo build 2>&1
```

Expected: compiles successfully (no usages yet, just verifying the structs are valid).

- [ ] **Step 3: Commit**

```bash
jj new -m "types: add VolumeResponse and SetVolumeRequest for volume API"
```

---

### Task 2: Add CiderClient volume methods

**Files:**
- Modify: `src/cider/mod.rs` (append two new methods before the `ClientError` enum)

- [ ] **Step 1: Add `get_volume()` method**

Insert after the `seek` method (before `pub enum ClientError`):

```rust
pub async fn get_volume(&self) -> Result<f64, ClientError> {
    let url = format!("{}/api/v1/playback/volume", self.base_url);
    let resp = self.client
        .get(&url)
        .headers(self.headers())
        .send()
        .await?;
    Ok(resp.json::<types::VolumeResponse>().await?.volume)
}
```

- [ ] **Step 2: Add `set_volume()` method**

Insert after `get_volume`:

```rust
pub async fn set_volume(&self, volume: f64) -> Result<(), ClientError> {
    let url = format!("{}/api/v1/playback/volume", self.base_url);
    self.client
        .post(&url)
        .headers(self.headers())
        .json(&types::SetVolumeRequest { volume })
        .send()
        .await?;
    Ok(())
}
```

- [ ] **Step 3: Build to verify methods compile**

```bash
cargo build 2>&1
```

Expected: compiles (methods aren't called yet, just verifying they're valid).

- [ ] **Step 4: Commit**

```bash
jj new -m "cider: add get_volume and set_volume HTTP methods"
```

---

### Task 3: Wire up MPRIS volume property

**Files:**
- Modify: `src/mpris/player.rs` — `PlayerState` struct, `Default` impl, `volume()` getter, add `set_volume()` setter, add `Duration` import

- [ ] **Step 1: Add `Duration` to imports**

In `src/mpris/player.rs`, change:

```rust
use std::time::Instant;
```

to:

```rust
use std::time::{Duration, Instant};
```

- [ ] **Step 2: Add `volume` and `volume_fetched_at` fields to `PlayerState`**

In the `PlayerState` struct, add two fields after `shuffle_mode`:

```rust
    pub volume: f64,
    pub volume_fetched_at: Instant,
```

- [ ] **Step 3: Update `Default` impl with volume defaults**

In `impl Default for PlayerState`, add volume defaults after `shuffle_mode: 0,`:

```rust
            volume: 1.0,
            volume_fetched_at: Instant::now(),
```

- [ ] **Step 4: Replace the hardcoded `volume()` getter with cached on-demand fetch**

Replace:

```rust
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }
```

with:

```rust
    #[zbus(property)]
    async fn volume(&self) -> f64 {
        let should_fetch = {
            let state = self.state.read().unwrap();
            state.volume_fetched_at.elapsed() > Duration::from_millis(500)
        };

        if should_fetch {
            match self.client.get_volume().await {
                Ok(vol) => {
                    let mut state = self.state.write().unwrap();
                    state.volume = vol;
                    state.volume_fetched_at = Instant::now();
                    vol
                }
                Err(e) => {
                    tracing::warn!("Failed to get volume: {:?}", e);
                    self.state.read().unwrap().volume
                }
            }
        } else {
            self.state.read().unwrap().volume
        }
    }
```

- [ ] **Step 5: Add `set_volume()` setter**

Insert after the `volume()` getter (before the `#[zbus(property(emits_changed_signal = "true"))]` on `position`):

```rust
    #[zbus(property)]
    async fn set_volume(&self, value: f64) {
        // Optimistic update — reflect the new value immediately
        {
            let mut state = self.state.write().unwrap();
            state.volume = value;
            state.volume_fetched_at = Instant::now();
        }

        // Fire-and-forget to Cider
        if let Err(e) = self.client.set_volume(value).await {
            tracing::warn!("Failed to set volume: {:?}", e);
        }
    }
```

- [ ] **Step 6: Build to verify everything compiles**

```bash
cargo build 2>&1
```

Expected: compiles cleanly.

- [ ] **Step 7: Commit**

```bash
jj new -m "mpris: implement volume get/set with cached on-demand fetch"
```

---

### Task 4: Bump minor version

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Bump version from `0.1.0` to `0.2.0`**

In `Cargo.toml`, change:

```toml
version = "0.1.0"
```

to:

```toml
version = "0.2.0"
```

- [ ] **Step 2: Build to verify version change is valid**

```bash
cargo build 2>&1
```

Expected: compiles, `cargo metadata` would show `0.2.0`.

- [ ] **Step 3: Commit**

```bash
jj new -m "chore: bump version to 0.2.0"
```

---

### Manual Verification

After implementing, run with Cider open:

```bash
# Check current volume
playerctl -p cider-mpris volume

# Set volume to 50%
playerctl -p cider-mpris volume 0.5

# Set volume to 100%
playerctl -p cider-mpris volume 1.0

# Set volume to 0 (mute)
playerctl -p cider-mpris volume 0.0

# Verify read-back reflects the set value
playerctl -p cider-mpris volume
```

**Expected behavior:**
- `playerctl volume` prints the current Cider volume (e.g., `0.5`)
- Setting volume immediately reflects in Cider's UI slider
- Reading volume after a set returns the new value (via optimistic cache)
- Reading volume when Cider is closed returns the last known value (no crash)
