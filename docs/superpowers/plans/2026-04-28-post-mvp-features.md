# Post-MVP Features — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Five post-MVP features: D-Bus name lifecycle, repeat/shuffle controls, seek support, systemd service, and flake.nix with home-manager module.

**Architecture:** Tasks 1-3 modify the existing bridge codebase (CiderClient, Player interface, polling loop). Tasks 4-5 add standalone config/build files. Execute in this order to avoid merge conflicts: repeat/shuffle (2) → seek (3) → name lifecycle (1) → systemd (4) → flake (5).

**Tech Stack:** Rust, zbus 4, reqwest, serde, systemd user units, Nix flakes

---

## File Structure Mapping

```
Cargo.toml            # Task 3: add serde Serialize import if needed
src/
├── main.rs           # Task 1: release_name/request_name calls
│                     # Task 2: repeat/shuffle state polling + signal emission
├── cider/
│   ├── mod.rs        # Tasks 2+3: new API methods (get_repeat_mode, toggle_repeat, get_shuffle_mode, toggle_shuffle, seek)
│   └── types.rs      # Task 2: RepeatModeResponse, ShuffleModeResponse types
│                     # Task 3: SeekRequest type
└── mpris/
    └── player.rs     # Task 2: loop_status/shuffle properties + setters, repeat_mode/shuffle_mode fields
                      # Task 3: can_seek, seek, set_position handlers
cider-mpris.service   # Task 4: systemd user service file
flake.nix             # Task 5: Nix flake
home-module.nix       # Task 5: home-manager module
```

---

## Task Decomposition

### Task 1: D-Bus Name Lifecycle

**Dependencies:** None (standalone main.rs edit)
**Files:**
- Modify: `src/main.rs`

**Why:** Release the D-Bus name when Cider becomes unavailable so widgets hide the player. Re-request on recovery.

- [ ] **Step 1: Add release_name call in unavailability transition**

In `src/main.rs`, find the unavailability block (where `cider_available` transitions from true to false). After emitting signals and before `prev_status = Some(PlaybackStatus::Stopped)`, add:

```rust
let _ = conn_for_signals
    .release_name("org.mpris.MediaPlayer2.cider")
    .await;
```

The block should look like:
```rust
if cider_available {
    tracing::warn!("Cider became unavailable (timeout or connection error)");
    cider_available = false;
    
    // ... existing state update + signal emission ...
    
    // Release D-Bus name so widgets hide the player
    let _ = conn_for_signals
        .release_name("org.mpris.MediaPlayer2.cider")
        .await;
    
    prev_status = Some(PlaybackStatus::Stopped);
    prev_metadata_key = None;
}
```

- [ ] **Step 2: Add request_name call in recovery**

Find the recovery block (where `!cider_available` after successful `is_playing()`). Add `request_name` after setting `cider_available = true`:

```rust
if !cider_available {
    tracing::info!("Cider is now available");
    cider_available = true;
    
    // Re-request D-Bus name so widgets rediscover the player
    if let Err(e) = conn_for_signals
        .request_name("org.mpris.MediaPlayer2.cider")
        .await
    {
        tracing::warn!("Failed to re-request D-Bus name: {:?}", e);
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero errors

- [ ] **Step 4: Commit**

```bash
jj new
jj describe -m "mpris: release D-Bus name when Cider unavailable, re-request on recovery"
```

---

### Task 2: Repeat & Shuffle Controls

**Dependencies:** Task 1 (they share main.rs but touch different regions)
**Files:**
- Modify: `src/cider/types.rs`
- Modify: `src/cider/mod.rs`
- Modify: `src/mpris/player.rs`
- Modify: `src/main.rs`

**Why follow Task 1:** Tasks 1 and 2 both touch main.rs but in different regions. Running Task 1 first keeps the diff clean.

- [ ] **Step 1: Add response types to `src/cider/types.rs`**

Append to the file:
```rust
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
```

- [ ] **Step 2: Add API methods to `src/cider/mod.rs`**

Append inside `impl CiderClient` block:
```rust
pub async fn get_repeat_mode(&self) -> Result<u8, ClientError> {
    let url = format!("{}/api/v1/playback/repeat-mode", self.base_url);
    let resp = self.client
        .get(&url)
        .headers(self.headers())
        .send()
        .await?;
    Ok(resp.json::<RepeatModeResponse>().await?.repeat_mode)
}

pub async fn get_shuffle_mode(&self) -> Result<u8, ClientError> {
    let url = format!("{}/api/v1/playback/shuffle-mode", self.base_url);
    let resp = self.client
        .get(&url)
        .headers(self.headers())
        .send()
        .await?;
    Ok(resp.json::<ShuffleModeResponse>().await?.shuffle_mode)
}

pub async fn toggle_repeat(&self) -> Result<(), ClientError> {
    let url = format!("{}/api/v1/playback/toggle-repeat", self.base_url);
    self.client.post(&url).headers(self.headers()).send().await?;
    Ok(())
}

pub async fn toggle_shuffle(&self) -> Result<(), ClientError> {
    let url = format!("{}/api/v1/playback/toggle-shuffle", self.base_url);
    self.client.post(&url).headers(self.headers()).send().await?;
    Ok(())
}
```

- [ ] **Step 3: Add fields to `PlayerState` in `src/mpris/player.rs`**

In the `PlayerState` struct, add two fields after `position_snapshot_at`:
```rust
pub repeat_mode: u8,
pub shuffle_mode: u8,
```

Update `Default` impl to include:
```rust
repeat_mode: 0,
shuffle_mode: 0,
```

- [ ] **Step 4: Replace loop_status and shuffle properties in `src/mpris/player.rs`**

Find the hardcoded `loop_status` property getter (currently returns `"None".to_string()`) and replace with:
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
    if let Err(e) = self.client.toggle_repeat().await {
        tracing::warn!("Toggle repeat failed: {:?}", e);
    }
}
```

Replace the hardcoded `shuffle` property getter (currently returns `false`) and add setter:
```rust
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

Note: Remove the old `#[zbus(property)]` `set_loop_status` and `set_shuffle` if they exist as separate stubs. The new code replaces entirely.

- [ ] **Step 5: Poll repeat/shuffle in main.rs polling loop**

In `src/main.rs`, in the `Ok(Some(info))` branch where state is updated, add repeat/shuffle fetching alongside the now-playing update. After reading `new_status` and the metadata block, add:

In the state update block (inside the `{ let mut s = ... }` scope), after setting `position_snapshot_at`:
```rust
// Fetch repeat and shuffle modes
let (repeat_mode, shuffle_mode) = {
    let rm = client_for_poll.get_repeat_mode().await.unwrap_or(0);
    let sm = client_for_poll.get_shuffle_mode().await.unwrap_or(0);
    (rm, sm)
};

s.repeat_mode = repeat_mode;
s.shuffle_mode = shuffle_mode;
```

Then after the metadata `/ emit signals` section, also emit signals for loop_status and shuffle if they changed. Track previous values:

Add new tracking variables at the top of the spawned task:
```rust
let mut prev_repeat_mode: Option<u8> = None;
let mut prev_shuffle_mode: Option<u8> = None;
```

In the signal emission section (after `drop(s)`), add:
```rust
if prev_repeat_mode != Some(repeat_mode) {
    let status_str = match repeat_mode {
        1 => "Track",
        2 => "Playlist",
        _ => "None",
    };
    let _ = emit_properties_changed(
        &conn_for_signals,
        "LoopStatus",
        Value::new(status_str),
    ).await;
    prev_repeat_mode = Some(repeat_mode);
}

if prev_shuffle_mode != Some(shuffle_mode) {
    let _ = emit_properties_changed(
        &conn_for_signals,
        "Shuffle",
        Value::new(shuffle_mode != 0),
    ).await;
    prev_shuffle_mode = Some(shuffle_mode);
}
```

When Cider becomes unavailable, reset these to `None` alongside `prev_metadata_key = None`:
```rust
prev_repeat_mode = None;
prev_shuffle_mode = None;
```

- [ ] **Step 6: Build and verify**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero errors

- [ ] **Step 7: Commit**

```bash
jj new
jj describe -m "mpris: add repeat/shuffle controls via Cider RPC API"
```

---

### Task 3: Seek Support

**Dependencies:** None (touches different regions from Task 2 in the same files)
**Files:**
- Modify: `src/cider/types.rs`
- Modify: `src/cider/mod.rs`
- Modify: `src/mpris/player.rs`

- [ ] **Step 1: Add SeekRequest type to `src/cider/types.rs`**

Append:
```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct SeekRequest {
    pub position: f64,
}
```

- [ ] **Step 2: Add seek method to `src/cider/mod.rs`**

Append inside `impl CiderClient`:
```rust
pub async fn seek(&self, position_seconds: f64) -> Result<(), ClientError> {
    let url = format!("{}/api/v1/playback/seek", self.base_url);
    self.client
        .post(&url)
        .headers(self.headers())
        .json(&types::SeekRequest { position: position_seconds })
        .send()
        .await?;
    Ok(())
}
```

- [ ] **Step 3: Update seek handlers in `src/mpris/player.rs`**

Change `can_seek` from `false` to `true`:
```rust
#[zbus(property)]
fn can_seek(&self) -> bool {
    true
}
```

Replace the `seek` stub with:
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
```

Replace the `set_position` stub with:
```rust
async fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) {
    let target_seconds = (position.max(0)) as f64 / 1_000_000.0;
    if let Err(e) = self.client.seek(target_seconds).await {
        tracing::warn!("SetPosition failed: {:?}", e);
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1`
Expected: SUCCESS with zero errors

- [ ] **Step 5: Commit**

```bash
jj new
jj describe -m "mpris: add seek support via Cider /seek API"
```

---

### Task 4: Systemd User Service

**Dependencies:** None (standalone config file)
**Files:**
- Create: `cider-mpris.service`

- [ ] **Step 1: Create `cider-mpris.service`**

Create file in repo root with contents:
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

- [ ] **Step 2: Commit**

```bash
jj new
jj describe -m "systemd: add user service file for cider-mpris bridge"
```

---

### Task 5: Flake.nix + Home-Manager Module

**Dependencies:** None (standalone build files)
**Files:**
- Create: `flake.nix`
- Create: `home-module.nix`

- [ ] **Step 1: Create `flake.nix`**

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
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl ];
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [ cargo rustc rust-analyzer pkg-config openssl ];
        };
      }
    ) // {
      homeModules.cider-mpris = import ./home-module.nix;
    };
}
```

- [ ] **Step 2: Create `home-module.nix`**

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

- [ ] **Step 3: Verify flake structure**

Run: `nix flake check 2>&1`
Expected: Validates successfully (may fail on first build if cargoHash needs computing, but structure should pass)

- [ ] **Step 4: Commit**

```bash
jj new
jj describe -m "nix: add flake.nix and home-manager module"
```

---

## Success Criteria

1. Kill Cider → player disappears from `playerctl -l` and DMS widget
2. Restart Cider → player reappears, shows current track
3. Press shuffle in DMS → Cider shuffle toggles, widget updates within 1s
4. Press repeat in DMS → Cider repeat cycles, widget updates within 1s
5. Scrub position in DMS → Cider seeks to that position
6. `systemctl --user enable --now cider-mpris` starts the bridge
7. `nix build .#` produces working binary (after adding to flake inputs)
