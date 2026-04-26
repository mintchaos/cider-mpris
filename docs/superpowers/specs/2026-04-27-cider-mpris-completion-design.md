# Cider-MPRIS Bridge — Completion Design

**Goal:** Bring the existing MPRIS bridge from a working skeleton to fully compliant, widget-ready operation. Fix signal emission so desktop widgets (Quickshell DMS, waybar, etc.) see real-time updates; add position interpolation for smooth progress bars; handle Cider availability gracefully; and clean up known bugs.

**Scope:** All items below. Systemd service is a separate followup.

---

## 1. Architecture

Three-layer structure remains unchanged:

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  D-Bus      │     │  Polling Loop    │     │  Cider API      │
│  (zbus)     │◄────│  (tokio::spawn)  │────►│  (HTTP/reqwest) │
│             │     │                  │     │                 │
│  Properties ──────┤ compare old/new  │     │  localhost:10767 │
│  Changed    │     │  emit on change  │     │                 │
└─────────────┘     └──────────────────┘     └─────────────────┘
```

The only structural change: the polling loop gains an `InterfaceRef<Player>` through which it emits `PropertiesChanged` signals when a tracked property differs from the previous poll.

---

## 2. PropertiesChanged Signal Emission (core fix)

**The problem:** The polling loop writes into `Arc<RwLock<PlayerState>>` but never emits D-Bus signals. Widgets see stale data from their initial query.

**The fix:**

After registering the `Player` interface on the object server, obtain an `InterfaceRef<Player>`:

```rust
let player_iface = object_server.interface_ref::<Player>().await?;
```

Clone this into the polling loop (same pattern already used for `conn_for_signals`).

On each poll iteration, after updating `PlayerState`, compare old vs. new for each `emits_changed_signal` property:

- **`PlaybackStatus`**: If changed, call `player_iface.playback_status_changed().await?`
- **`Metadata`**: If the set of relevant fields changed (title, artist, album, artUrl, length), call `player_iface.metadata_changed(new_value).await?`
- **`Position`**: Don't emit on every poll tick — only if playback status transitions (emitting position on every 500ms would be noisy). Widgets will query `Position` on-demand (see interpolation below).

Signal emission happens after the `PlayerState` lock is released, so no deadlock risk.

**Transition behavior for Cider-unavailable:** When transitioning to `Stopped` due to connection failure, emit `PlaybackStatus` once. Do **not** emit on subsequent polls while Cider is still down — avoid signal storms.

---

## 3. Position Interpolation

**The problem:** `Position` is snapshotted from `current_playback_time` in the API poll and remains frozen between polls.

**The fix:** Add `snapshot_at: Instant` to `PlayerState`. On each poll, store:

```
position_snapshot = current_playback_time_ms * 1000  // microseconds
snapshot_at        = Instant::now()
```

The `position` property getter computes on-read:

```rust
fn position(&self) -> i64 {
    let state = self.state.read().unwrap();
    if state.playback_status == PlaybackStatus::Playing {
        let elapsed = snapshot_at.elapsed().as_micros() as i64;
        state.position_snapshot + elapsed
    } else {
        state.position_snapshot
    }
}
```

Also account for `PlaybackRate` (always 1.0 currently):

```rust
elapsed = (elapsed as f64 * rate) as i64
```

This gives smooth, sub-poll-interval position without any background timer.

---

## 4. Cider Availability Handling

**The problem:** When Cider isn't running, the bridge should be silent but remain discoverable on D-Bus.

**States:**

| State | Trigger | D-Bus behavior |
|-------|---------|----------------|
| Active | API responds | Normal — `Playing` or `Paused`, full metadata |
| Unavailable | Connection refused / timeout / 5xx | Set `PlaybackStatus` to `Stopped`, clear metadata. Emit `PropertiesChanged` **only on transition**. Sleep longer (2s) between polls. |
| Recovery | API responds after period of unavailability | Restore normal polling interval (500ms), emit fresh status + metadata |

**Key detail:** The D-Bus name `org.mpris.MediaPlayer2.cider` stays registered the entire time. This matches the MPRIS convention: players that exit still have their name on the bus — they just report `Stopped`.

---

## 5. Runtime Drop Panic Fix

**The problem:** `reqwest::blocking::Client` spawns an internal tokio runtime. When the process panics or exits, the `Drop` of this client runs inside the `#[tokio::main]` async context, causing: "Cannot drop a runtime in a context where blocking is not allowed."

**The fix:** Switch `CiderClient` from `reqwest::blocking::Client` to async `reqwest::Client`. The D-Bus method handlers (which zbus requires to be synchronous) bridge to async via:

```rust
fn play(&self) {
    let handle = tokio::runtime::Handle::current();
    if let Err(e) = handle.block_on(self.client.play()) {
        tracing::warn!("Play failed: {:?}", e);
    }
}
```

Remove all `*_blocking` methods — only async methods remain on `CiderClient`. The polling loop already runs in an async context, so those calls become natural async calls (remove the `*_blocking` suffix from those call sites).

**Feature flag update:** In `Cargo.toml`, remove the `blocking` feature from `reqwest`:

```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
```

---

## 6. Cleanup

- Remove unused `conn_for_signals` (replaced by `InterfaceRef`).
- Remove `ClientError` enum's internal wrapper — or keep it but actually use the inner error in log messages.
- Remove dead async wrapper methods (the `play()`, `pause()`, etc. that were never called).
- Fix all compiler warnings so `cargo build` is clean.

---

## 7. Verification Plan

These are manual tests to run after implementation:

| # | Test | Expected |
|---|------|----------|
| 1 | `cargo build` | Clean, zero warnings |
| 2 | Start bridge, Cider already playing | `playerctl -p cider status` → `Playing`; `playerctl metadata` shows track info |
| 3 | Change track in Cider | Within 1s: metadata updates (verify via `playerctl metadata`) |
| 4 | Pause via `playerctl -p cider pause` | `playerctl status` → `Paused`; DMS widget updates |
| 5 | Resume via `playerctl -p cider play` | `playerctl status` → `Playing`; DMS widget updates |
| 6 | Stop Cider completely | Bridge stays alive; `playerctl status` → `Stopped`; no repeated signal spam in logs |
| 7 | Restart Cider | Bridge recovers; `playerctl status` → `Playing`/`Paused` |
| 8 | Ctrl+C the bridge | Clean shutdown, no panic |
| 9 | DMS widget bar | All of the above reflected in the bar |
