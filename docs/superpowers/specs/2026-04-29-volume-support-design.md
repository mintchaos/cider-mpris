# Volume Support Design

**Date**: 2026-04-29
**Status**: Approved

## Overview

Add MPRIS volume get/set support to `cider-mpris`. Currently the `Volume` property in the `org.mpris.MediaPlayer2.Player` interface returns a hardcoded `1.0`. The Cider HTTP API already has `GET /api/v1/playback/volume` and `POST /api/v1/playback/volume` endpoints (both use a `0.0`–`1.0` range).

## Design

Cached on-demand fetch with optimistic writes and rate limiting. Volume is **not** added to the 500ms polling loop — it's fetched only when a D-Bus client reads the property, with a 500ms cache TTL to avoid rapid-fire HTTP calls from widget polling.

### Components Changed

- **`src/cider/types.rs`** — new `VolumeResponse` deserialization struct
- **`src/cider/mod.rs`** — new `get_volume()` and `set_volume(f64)` methods on `CiderClient`
- **`src/mpris/player.rs`** — `PlayerState` gains `volume` and `volume_fetched_at` fields; `volume` getter and `set_volume` setter replaced with live implementations

### Data Flow

#### Read (volume property getter)

```
D-Bus client reads Volume
  → player.rs volume() getter
    → if cache age < 500ms → return cached volume
    → else → CiderClient::get_volume() → GET /api/v1/playback/volume
      → on success: update PlayerState.volume + volume_fetched_at, return value
      → on failure: log warning, return stale cached volume (last known)
```

#### Write (set_volume property setter)

```
D-Bus client sets Volume to new_value
  → player.rs set_volume()
    → immediately: PlayerState.volume = new_value (optimistic)
    → fire-and-forget: CiderClient::set_volume(new_value) → POST /api/v1/playback/volume
      → on failure: log warning (cache already reflects desired value)
```

### New Types

```rust
// types.rs
#[derive(Debug, Clone, Deserialize)]
pub struct VolumeResponse {
    pub volume: f64,
}
```

### New CiderClient Methods

```rust
// mod.rs
pub async fn get_volume(&self) -> Result<f64, ClientError> {
    // GET /api/v1/playback/volume
    // Returns volume field (0.0–1.0)
}

pub async fn set_volume(&self, volume: f64) -> Result<(), ClientError> {
    // POST /api/v1/playback/volume with body {"volume": <volume>}
}
```

### PlayerState Changes

Two new fields:
- `volume: f64` — default `1.0`
- `volume_fetched_at: Instant` — for cache TTL check

### Player Interface Changes

- `volume()` getter → live implementation with cache + fetch
- `set_volume(value: f64)` setter → optimistic write + fire-and-forget POST

## What's NOT Done

- Volume is **not** polled in the 500ms main loop
- No `PropertiesChanged` signals are emitted for volume changes (MPRIS clients that display volume poll it on their own schedule; our 500ms cache TTL handles that gracefully)
- No mute support (not in Cider API)
- Volume changes made inside Cider (not via MPRIS) won't be reflected until a D-Bus client reads the property after the cache expires — this is an acceptable trade-off

## Testing

- `playerctl -p cider-mpris volume` — should return Cider's current volume
- `playerctl -p cider-mpris volume 0.5` — should set Cider volume to 50%
- `playerctl -p cider-mpris volume 0.0` — should mute
- `playerctl -p cider-mpris volume 1.0` — should set to full
- Volume read should return last known value when Cider is unavailable
