// persistence.rs — Atomic, crash-safe disk persistence.
// Saves to ~/.kore-self/<owner>/memories.kore.json
// Atomic write: write to .tmp → rename (never corrupts on crash)

use std::fs;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

use crate::Memory;
use crate::identity::IdentityModel;
use crate::consciousness::ConsciousnessState;
use crate::dream::DreamEngine;
use crate::shadow::ShadowObserver;
use crate::predictive::PredictiveEngine;
use crate::social::VoiceEngine;
use crate::mortality::MortalityEngine;
use crate::evolution::EvolutionEngine;
use crate::broadcast::BroadcastEngine;
use crate::assistant::AssistantEngine;

const SAVE_VERSION: u32 = 8;  // bumped: assistant (human mode) added

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version:       u32,
    saved_at:      String,
    owner:         String,
    memories:      Vec<Memory>,
    identity:      IdentityModel,
    consciousness: ConsciousnessState,
    #[serde(default)]
    dream:         DreamEngine,
    #[serde(default)]
    shadow:        ShadowObserver,
    #[serde(default)]
    predictive:    PredictiveEngine,
    #[serde(default)]
    social:        VoiceEngine,
    #[serde(default)]
    mortality:     MortalityEngine,
    #[serde(default)]
    evolution:     EvolutionEngine,
    #[serde(default)]
    broadcast:     BroadcastEngine,
    #[serde(default)]
    assistant:     AssistantEngine,
    next_id:       u64,
}

/// Full path to the save file for this owner.
pub fn data_path(owner: &str) -> PathBuf {
    home_dir()
        .join(".kore-self")
        .join(owner)
        .join("memories.kore.json")
}

fn home_dir() -> PathBuf {
    // Windows: USERPROFILE. Linux/macOS: HOME.
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Save full state atomically.
/// Write to .tmp → rename — guarantees no half-written file on crash.
pub fn save(
    owner:         &str,
    memories:      &[Memory],
    identity:      &IdentityModel,
    consciousness: &ConsciousnessState,
    dream:         &DreamEngine,
    shadow:        &ShadowObserver,
    predictive:    &PredictiveEngine,
    social:        &VoiceEngine,
    mortality:     &MortalityEngine,
    evolution:     &EvolutionEngine,
    broadcast:     &BroadcastEngine,
    assistant:     &AssistantEngine,
    next_id:       u64,
) -> std::io::Result<()> {
    let path = data_path(owner);
    let parent = path.parent().expect("path has parent");
    fs::create_dir_all(parent)?;

    let sf = SaveFile {
        version:       SAVE_VERSION,
        saved_at:      crate::now(),
        owner:         owner.to_string(),
        memories:      memories.to_vec(),
        identity:      identity.clone(),
        consciousness: consciousness.clone(),
        dream:         dream.clone(),
        shadow:        shadow.clone(),
        predictive:    predictive.clone(),
        social:        social.clone(),
        mortality:     mortality.clone(),
        evolution:     evolution.clone(),
        broadcast:     broadcast.clone(),
        assistant:     assistant.clone(),
        next_id,
    };

    let json = serde_json::to_string_pretty(&sf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Atomic: write tmp, then rename into place
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, json.as_bytes())?;
    fs::rename(&tmp_path, &path)?;

    Ok(())
}

/// Load full state from disk.
pub fn load(owner: &str) -> Option<(Vec<Memory>, IdentityModel, ConsciousnessState, DreamEngine, ShadowObserver, PredictiveEngine, VoiceEngine, MortalityEngine, EvolutionEngine, BroadcastEngine, AssistantEngine, u64)> {
    let path = data_path(owner);
    let bytes = fs::read(&path).ok()?;
    let sf: SaveFile = serde_json::from_slice(&bytes)
        .map_err(|e| eprintln!("[kore-self] Warning: save file corrupt ({e}), starting fresh"))
        .ok()?;
    Some((sf.memories, sf.identity, sf.consciousness, sf.dream, sf.shadow, sf.predictive, sf.social, sf.mortality, sf.evolution, sf.broadcast, sf.assistant, sf.next_id))
}

/// Disk usage stats.
pub fn disk_stats(owner: &str) -> serde_json::Value {
    let path = data_path(owner);
    let sz   = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    serde_json::json!({
        "path":       path.to_string_lossy(),
        "size_bytes": sz,
        "size_kb":    sz / 1024,
        "exists":     path.exists(),
    })
}
