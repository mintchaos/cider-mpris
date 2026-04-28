use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zbus::Connection;
use zbus::zvariant::Value;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cider;
mod mpris;

const MPRIS_NAME: &str = "org.mpris.MediaPlayer2.cider-mpris";

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
    
    // Don't request the well-known name yet — defer until Cider is actually playing.
    // This prevents an empty player widget from appearing when Cider is closed.
    
    let object_server = conn.object_server();
    
    // Register Root interface
    object_server
        .at("/org/mpris/MediaPlayer2", Root::new())
        .await?;
    
    // Register Player interface
    let player = Player::new(
        Arc::clone(&cider_client),
        Arc::clone(&player_state),
    );
    object_server
        .at("/org/mpris/MediaPlayer2", player)
        .await?;

    // Polling task with property change detection and signal emission
    let state_for_poll = Arc::clone(&player_state);
    let client_for_poll = Arc::clone(&cider_client);
    let conn_for_signals = conn.clone();

    tokio::spawn(async move {
        let active_interval = tokio::time::Duration::from_millis(500);
        let retry_interval = tokio::time::Duration::from_secs(2);
        let mut cider_available = true;
        let mut name_owned = false;
        let mut prev_status: Option<PlaybackStatus> = None;
        let mut prev_metadata_key: Option<String> = None;
        let mut prev_repeat_mode: Option<u8> = None;
        let mut prev_shuffle_mode: Option<u8> = None;
        
        loop {
            // --- Check playback status from Cider ---
            // Use a short timeout so unavailability is detected quickly
            let is_playing_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                client_for_poll.is_playing(),
            ).await;
            
            let is_playing = match is_playing_result {
                Ok(Ok(playing)) => playing,
                _ => {
                    // Cider became unavailable or check timed out
                    if cider_available {
                        tracing::warn!("Cider became unavailable (timeout or connection error)");
                        cider_available = false;
                        
                        let should_emit = {
                            let mut s = state_for_poll.write().unwrap();
                            let should = s.playback_status != PlaybackStatus::Stopped
                                || s.now_playing.is_some();
                            if should {
                                s.playback_status = PlaybackStatus::Stopped;
                                s.now_playing = None;
                            }
                            should
                        };
                        
                        if should_emit {
                            let _ = emit_properties_changed(
                                &conn_for_signals,
                                "PlaybackStatus",
                                Value::new("Stopped"),
                            ).await;
                            let _ = emit_properties_changed(
                                &conn_for_signals,
                                "Metadata",
                                Value::new(HashMap::<String, Value<'_>>::new()),
                            ).await;
                        }

                        // Release D-Bus name so widgets hide the player
                        if name_owned {
                            let _ = conn_for_signals
                                .release_name(MPRIS_NAME)
                                .await;
                            name_owned = false;
                        }
                        prev_status = Some(PlaybackStatus::Stopped);
                        prev_metadata_key = None;
                        prev_repeat_mode = None;
                        prev_shuffle_mode = None;
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
                    // Register D-Bus name now that something is playing
                    if !name_owned {
                        if let Err(e) = conn_for_signals
                            .request_name(MPRIS_NAME)
                            .await
                        {
                            tracing::warn!("Failed to request D-Bus name: {:?}", e);
                        } else {
                            name_owned = true;
                        }
                    }

                    // Extract repeat/shuffle from the now-playing response
                    let repeat_mode = info.repeat_mode;
                    let shuffle_mode = info.shuffle_mode;
                    
                    let (meta_key, metadata_changed, status_changed, metadata_for_signal) = {
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
                        s.repeat_mode = repeat_mode;
                        s.shuffle_mode = shuffle_mode;
                        
                        // Build metadata snapshot (while lock held)
                        let metadata_for_signal = build_metadata_map(&s);
                        
                        (meta_key, metadata_changed, status_changed, metadata_for_signal)
                    };
                    
                    // Emit signals outside the lock
                    if status_changed {
                        let status_str = new_status.as_str();
                        let _ = emit_properties_changed(
                            &conn_for_signals,
                            "PlaybackStatus",
                            Value::new(status_str),
                        ).await;
                        prev_status = Some(new_status);
                    }
                    
                    if metadata_changed {
                        let metadata_value = Value::new(metadata_for_signal);
                        let _ = emit_properties_changed(
                            &conn_for_signals,
                            "Metadata",
                            metadata_value,
                        ).await;
                        prev_metadata_key = Some(meta_key);
                    }
                    
                    // Emit loop/shuffle change signals
                    let current_repeat = {
                        let s = state_for_poll.read().unwrap();
                        s.repeat_mode
                    };
                    let current_shuffle = {
                        let s = state_for_poll.read().unwrap();
                        s.shuffle_mode
                    };
                    
                    if prev_repeat_mode != Some(current_repeat) {
                        let status_str = match current_repeat {
                            1 => "Track",
                            2 => "Playlist",
                            _ => "None",
                        };
                        let _ = emit_properties_changed(
                            &conn_for_signals,
                            "LoopStatus",
                            Value::new(status_str),
                        ).await;
                        prev_repeat_mode = Some(current_repeat);
                    }
                    
                    if prev_shuffle_mode != Some(current_shuffle) {
                        let _ = emit_properties_changed(
                            &conn_for_signals,
                            "Shuffle",
                            Value::new(current_shuffle != 0),
                        ).await;
                        prev_shuffle_mode = Some(current_shuffle);
                    }
                }
                Ok(None) => {
                    // Cider reports nothing playing — release D-Bus name so widget hides
                    if name_owned {
                        let _ = conn_for_signals
                            .release_name(MPRIS_NAME)
                            .await;
                        name_owned = false;
                    }

                    // Cider reports nothing playing — treat as Stopped
                    let should_emit = {
                        let mut s = state_for_poll.write().unwrap();
                        let should = s.playback_status != PlaybackStatus::Stopped
                            || s.now_playing.is_some();
                        if should {
                            s.playback_status = PlaybackStatus::Stopped;
                            s.now_playing = None;
                        }
                        should
                    };
                    
                    if should_emit {
                        let _ = emit_properties_changed(
                            &conn_for_signals,
                            "PlaybackStatus",
                            Value::new("Stopped"),
                        ).await;
                        let _ = emit_properties_changed(
                            &conn_for_signals,
                            "Metadata",
                            Value::new(HashMap::<String, Value<'_>>::new()),
                        ).await;
                        prev_status = Some(PlaybackStatus::Stopped);
                        prev_metadata_key = None;
                        prev_repeat_mode = None;
                        prev_shuffle_mode = None;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch now playing: {:?}", e);
                }
            }
            
            tokio::time::sleep(active_interval).await;
        }
    });

    tracing::info!("cider-mpris bridge running on D-Bus as {MPRIS_NAME}");
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

/// Emit a PropertiesChanged signal on the D-Bus connection.
async fn emit_properties_changed(
    conn: &Connection,
    property_name: &str,
    new_value: Value<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use zbus::Message;
    
    let mut changed_props = HashMap::new();
    changed_props.insert(property_name.to_string(), new_value);
    
    let invalidated_props: Vec<&str> = vec![];
    
    let body = (
        "org.mpris.MediaPlayer2.Player".to_string(),
        changed_props,
        invalidated_props,
    );
    
    let msg = Message::signal(
        "/org/mpris/MediaPlayer2",
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
    )?
    .build(&body)?;
    
    conn.send(&msg).await?;
    
    Ok(())
}
