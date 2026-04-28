# Cider-MPRIS Post-MVP Features — Design Spec

**Goal:** Five post-MVP improvements: D-Bus name lifecycle so the player disappears when Cider is unavailable, repeat/shuffle controls, seek support, a systemd user service, and a Nix flake with home-manager module.

**Scope:** Five independent features in one spec. Systemd + flake are dependency-free; the three code features build on the existing polling loop.

---

## 1. D-Bus Name Lifecycle

**Problem:** When Cider isn't running, the bridge stays on D-Bus reporting `Stopped`. Widgets still show it in their media list, creating visual noise.

**Solution:** Release the D-Bus name when Cider becomes unavailable, re-request on recovery.

### Implementation

In the polling loop's unavailability transition (first failed `is_playing()` after Cider was available):

```rust
// After setting state to Stopped and emitting signals:
let _ = conn_for_signals
    .release_name("org.mpris.MediaPlayer2.cider")
    .await;
```

On recovery (first successful `is_playing()` after unavailability):

```rust
if !cider_available {
    tracing::info!("Cider is now available");
    cider_available = true;
    if let Err(e) = conn_for_signals
        .request_name("org.mpris.MediaPlayer2.cider")
        .await
    {
        tracing::warn!("Failed to re-request D-Bus name: {:?}", e);
    }
}
```

Widgets see the player appear/disappear as Cider comes and goes. `conn_for_signals` is already cloned into the polling task — no new wiring needed.

**Edge cases:**
- If `release_name` fails (already gone): harmless, continue
- If `request_name` fails (another process owns it): log warning, try again on next poll

---

## 2. Repeat & Shuffle Controls

**Problem:** `LoopStatus` always returns `"None"`, `Shuffle` always returns `false`, and the setters are no-ops. Widget repeat/shuffle buttons don't work.

**Cider API:**
| Method | Endpoint | Behavior |
|--------|----------|----------|
| `GET /api/v1/playback/repeat-mode` | Returns `{ "repeatMode": 0\|1\|2 }` | 0=off, 1=repeat-one, 2=repeat-all |
| `POST /api/v1/playback/toggle-repeat` | Cycles: one→all→off | No request body |
| `GET /api/v1/playback/shuffle-mode` | Returns `{ "shuffleMode": 0\|1 }` | 0=off, 1=on |
| `POST /api/v1/playback/toggle-shuffle` | Toggles off↔on | No request body |

### Implementation

**`CiderClient` — four new methods:**

```rust
async fn get_repeat_mode(&self) -> Result<u8, ClientError>
async fn get_shuffle_mode(&self) -> Result<u8, ClientError>
async fn toggle_repeat(&self) -> Result<(), ClientError>
async fn toggle_shuffle(&self) -> Result<(), ClientError>
```

**`PlayerState` — two new fields:**
```rust
pub repeat_mode: u8,  // 0=off, 1=one, 2=all
pub shuffle_mode: u8, // 0=off, 1=on
```
Defaults: both `0`.

**Polling loop:** After `now_playing()` succeeds, also fetch `get_repeat_mode()` and `get_shuffle_mode()`. Compare with previous values; emit `PropertiesChanged` for `LoopStatus` / `Shuffle` when they differ.

**Player properties (replace hardcoded stubs):**
```rust
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
    // Cider's toggle-repeat cycles: one → all → off
    // The widget sends the desired target, but we can only toggle.
    // After toggling, the next poll reads the new mode and the widget updates.
    if let Err(e) = self.client.toggle_repeat().await {
        tracing::warn!("Toggle repeat failed: {:?}", e);
    }
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
```

**Widget flow:** User presses shuffle → toggle sent → next poll (≤500ms) reads new mode → widget updates. Same for repeat.

---

## 3. Seek Support

**Problem:** `can_seek` returns `false`, `seek()` and `set_position()` are no-ops. Widgets can't show seekable progress bars or let the user scrub.

**Cider API:** `POST /api/v1/playback/seek` with body `{ "position": <seconds> }`.

### Implementation

**`CiderClient` — one new method:**
```rust
#[derive(Serialize)]
struct SeekRequest {
    position: f64,
}

async fn seek(&self, position_seconds: f64) -> Result<(), ClientError> {
    let url = format!("{}/api/v1/playback/seek", self.base_url);
    self.client
        .post(&url)
        .headers(self.headers())
        .json(&SeekRequest { position: position_seconds })
        .send()
        .await?;
    Ok(())
}
```

Note: `CiderClient` will need `Cargo.toml` update to add `serde::Serialize` derive or use manual JSON construction. Since `serde` is already a dep and `types.rs` uses `Deserialize`, adding `Serialize` is trivial.

**Player changes:**
```rust
#[zbus(property)]
fn can_seek(&self) -> bool {
    true
}

fn seek(&self, offset: i64) {
    // offset is in microseconds, positive or negative
    let handle = tokio::runtime::Handle::current();
    // Read current interpolated position
    let current_pos = {
        let state = self.state.read().unwrap();
        if state.playback_status == PlaybackStatus::Playing {
            let elapsed = state.position_snapshot_at.elapsed().as_micros() as i64;
            state.position_snapshot_us + elapsed
        } else {
            state.position_snapshot_us
        }
    };
    let target_us = (current_pos + offset).max(0);
    let target_seconds = target_us as f64 / 1_000_000.0;
    if let Err(e) = handle.block_on(self.client.seek(target_seconds)) {
        tracing::warn!("Seek failed: {:?}", e);
    }
}

fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) {
    let handle = tokio::runtime::Handle::current();
    let target_seconds = (position.max(0)) as f64 / 1_000_000.0;
    if let Err(e) = handle.block_on(self.client.seek(target_seconds)) {
        tracing::warn!("SetPosition failed: {:?}", e);
    }
}
```

Wait — the Player methods were just converted to `async fn`. For seek, we need the current position which requires reading `PlayerState`. With async methods this works directly since we're on the tokio runtime:

```rust
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
```

---

## 4. Systemd User Service

**Problem:** Bridge must be started manually. Should autostart with the graphical session.

### Service file

`cider-mpris.service` (lives in repo root, installed to `~/.config/systemd/user/` by the flake):

```ini
[Unit]
Description=Cider MPRIS Bridge
After=graphical-session.target
Wants=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/cider-mpris
EnvironmentFile=%h/.config/cider-mpris/env
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

- `After=graphical-session.target` ensures D-Bus session bus is available
- `%h` expands to the user's home directory
- `EnvironmentFile` keeps the token out of the Nix store
- `Restart=on-failure` recovers from crashes

The env file (`~/.config/cider-mpris/env`) contains:
```
CIDER_RPC_TOKEN=your_token_here
```

---

## 5. Flake.nix + Home-Manager Module

**Problem:** No `nix build` or `nix run`, no declarative home-manager integration.

### flake.nix

```nix
{
  description = "MPRIS bridge for Cider music player";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "cider-mpris";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          # vendor openssl since we use the vendored feature
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [ cargo rustc rust-analyzer ];
        };
      }
    ) // {
      homeModules.cider-mpris = import ./home-module.nix;
    };
}
```

### home-module.nix

```nix
{ config, lib, pkgs, ... }:

let cfg = config.services.cider-mpris;
in {
  options.services.cider-mpris = {
    enable = lib.mkEnableOption "Cider MPRIS bridge";

    package = lib.mkPackageOption pkgs "cider-mpris" { };

    rpcTokenFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to file containing CIDER_RPC_TOKEN";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."cider-mpris/env".text =
      if cfg.rpcTokenFile != null
      then "CIDER_RPC_TOKEN=${builtins.readFile cfg.rpcTokenFile}"
      else "# Add your CIDER_RPC_TOKEN here\nCIDER_RPC_TOKEN=";

    systemd.user.services.cider-mpris = {
      Unit = {
        Description = "Cider MPRIS Bridge";
        After = "graphical-session.target";
        Wants = "graphical-session.target";
      };
      Service = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/cider-mpris";
        EnvironmentFile = "${config.xdg.configHome}/cider-mpris/env";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
```

**Usage in home-manager:**
```nix
{ inputs, ... }: {
  imports = [ inputs.cider-mpris.homeModules.cider-mpris ];
  services.cider-mpris = {
    enable = true;
    rpcTokenFile = /home/xian/secrets/cider-rpc-token;
  };
}
```

### Build verification

```bash
nix build .#        # builds the binary
nix flake check     # validates flake structure
```

---

## 6. Verification

| # | Test | Expected |
|---|------|----------|
| 1 | Kill Cider, bridge running | Player disappears from `playerctl -l` and DMS widget |
| 2 | Restart Cider | Player reappears, shows current track |
| 3 | Press shuffle in DMS widget | Cider shuffle toggles, widget updates within 1s |
| 4 | Press repeat in DMS widget | Cider repeat cycles, widget updates within 1s |
| 5 | Scrub position in widget (seek) | Cider seeks to that position |
| 6 | `systemctl --user start cider-mpris` | Service starts, D-Bus name appears |
| 7 | `nix flake check` | Validates without errors |
| 8 | `nix build` | Produces working binary |
