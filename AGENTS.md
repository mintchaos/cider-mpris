# cider-mpris — MPRIS Bridge for Cider

## Project Overview

This project creates an MPRIS (Media Player Remote Interfacing Standard) bridge that allows system-wide media control of the [Cider](https://cider.sh/) music player on Linux. MPRIS enables integration with desktop environments, media keybindings, and tools like playerctl.

**Repository**: Jujutsu (jj) version-controlled, not Git.

## Architecture Notes

### MPRIS Compliance

The implementation should conform to [MPRIS v2.2](https://specifications.freedesktop.org/mpris-spec/latest/) specification:

| Interface | Required |
|-----------|----------|
| `org.mpris.MediaPlayer2` | Yes — Root object with `Raise`, `CanQuit`, `CanSetFullscreen` |
| `org.mpris.MediaPlayer2.Player` | Yes — Playback control: `Play`, `Pause`, `Stop`, `Next`, `Previous`, `Seek` |
| `org.mpris.MediaPlayer2.Playlists` | Optional — Playlist management |

### Cider Communication

The bridge needs to communicate with Cider via its IPC/API. Investigate:
- WebSocket/HTTP API endpoints
- Local port or socket
- IPC mechanism Cider exposes

### Key Implementation Patterns

1. **D-Bus Service Registration**: Register as `org.mpris.MediaPlayer2.cider` on the session bus
2. **Property Change Notifications**: Emit `PropertiesChanged` signals for metadata, playback status
3. **Seek Support**: Track position with `Position` property (microseconds)
4. **Metadata Format**: Use `mpris:trackid`, `mpris:length`, `xesam:artist`, `xesam:title`, etc.

## Conventions

### Project Structure

```
src/
├── main.rs           # Entry point, D-Bus service registration
├── player.rs         # Player state management, Cider communication
├── mpris/
│   ├── mod.rs        # MPRIS interface implementations
│   ├── root.rs       # org.mpris.MediaPlayer2
│   └── player.rs     # org.mpris.MediaPlayer2.Player
└── cider/
    ├── mod.rs        # Cider IPC client
    └── types.rs      # Cider API types
```

## Development Guidelines

### Testing Strategy

1. **Unit Tests**: Test individual modules, especially state machine logic
2. **Integration Tests**: Start a mock Cider server or use recorded responses
3. **D-Bus Compliance**: Verify with `mpris-explorer` or `dbus-monitor`
4. **Manual Testing**: `playerctl -p cider status`, `playerctl -p cider play-pause`

### Common Commands

```bash
# Build
cargo build

# Run
cargo run

# Test
cargo test

# Check MPRIS bus (with service running)
dbus-send --session --dest=org.mpris.MediaPlayer2.cider --type=method_call --print-reply /org/mpris/MediaPlayer2 org.freedesktop.DBus.Properties.Get string:'org.mpris.MediaPlayer2' string:'CanQuit'

# List available players
playerctl -l
```

### Error Handling

- Log connection failures to Cider with clear messages
- Handle Cider not running gracefully (MPRIS allows this)
- Validate all D-Bus property accessors

## MPRIS Properties Reference

| Property | Type | Description |
|----------|------|-------------|
| `PlaybackStatus` | `s` | "Playing", "Paused", "Stopped" |
| `LoopStatus` | `s` | "None", "Track", "Playlist" |
| `Rate` | `d` | Playback rate (1.0 normal) |
| `Shuffle` | `b` | Shuffle state |
| `Volume` | `d` | 0.0 to 1.0 |
| `Position` | `t` | Microseconds from start |
| `Metadata` | `a{sv}` | Track info dict |
| `CanGoNext` | `b` | Next track available |
| `CanGoPrevious` | `b` | Previous track available |
| `CanPlay` | `b` | Can start playback |
| `CanPause` | `b` | Can pause playback |
| `CanSeek` | `b` | Can seek within track |
| `CanControl` | `b` | Basic controls available |

## Important Notes

1. **jj Workflow**: Use `jj` instead of `git`. Key commands: `jj log`, `jj new`, `jj describe`, `jj push`
2. **D-Bus Session**: Ensure running on a session bus (not system bus for desktop players)
3. **Cider Compatibility**: Verify API version with running Cider instance
4. **Single Instance**: Handle case where another instance might already own the MPRIS name

## Resources

- [MPRIS Specification](https://specifications.freedesktop.org/mpris-spec/latest/)
- [zbus Documentation](https://docs.rs/zbus/latest/zbus/)
- [Cider API Documentation](https://cider.sh/)
