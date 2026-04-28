# AGENTS.md

## Project

MPRIS bridge for the [Cider](https://cider.sh/) music player on Linux. Polls Cider's local HTTP API (localhost:10767) and exposes MPRIS D-Bus interfaces (`org.mpris.MediaPlayer2.cider-mpris`) so desktop widgets, media keys, and `playerctl` can control Cider playback.

## Key Files

```
src/
├── main.rs           # Entry point, D-Bus setup, polling loop with signal emission
├── cider/
│   ├── mod.rs        # Async HTTP client for Cider RPC API
│   └── types.rs      # Serde types for API requests/responses
└── mpris/
    ├── mod.rs        # Module declarations
    ├── root.rs       # org.mpris.MediaPlayer2 interface
    └── player.rs     # org.mpris.MediaPlayer2.Player interface + state
```

## Cider API

Base URL: `http://localhost:10767/api/v1/playback/`
Auth: `apptoken` header from `CIDER_RPC_TOKEN` env var

Key endpoints: `is-playing`, `now-playing`, `play`, `pause`, `playpause`, `next`, `previous`, `seek`, `repeat-mode`, `shuffle-mode`, `toggle-repeat`, `toggle-shuffle`

Full docs: `docs/cider-rpc-api.md`

## Build & Run

```bash
# Set token in .env
echo 'CIDER_RPC_TOKEN=your_token' > .env

cargo build --release
./target/release/cider-mpris
```

Use `playerctl -p cider-mpris <command>` to test. `RUST_LOG=debug` for verbose output.

## Architecture Notes

- Polling loop in `main.rs` fetches playback state every 500ms, diffs against previous values, emits `PropertiesChanged` D-Bus signals on change.
- D-Bus name `org.mpris.MediaPlayer2.cider-mpris` is only requested when Cider is actually playing/paused — released when stopped or unavailable.
- Position interpolates from API snapshot + wall-clock elapsed time for smooth widget display.
- All D-Bus method handlers are `async fn` (zbus with `tokio` feature).

## jj Workflow

```bash
jj log         # view history
jj new -m "..." # new change
jj describe -m "..." # set description
```
