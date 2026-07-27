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
use crate::action::ActionBridge;
use crate::goals::GoalEngine;
use crate::becoming::{NeedEngine, TemporalSelf, Story, BecomingEngine,
    EvolutionTracker, Worldview, NarrativeIdentity, ValuesEngine, MeaningEngine,
    RealityEngine, LegacyEngine, ResearchEngine};
use kore_federation::FederationEngine;

const SAVE_VERSION: u32 = 9;  // bumped: KORE-BECOMING layer added

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
    // ── KORE-BECOMING layer (persisted since v9) ──────────────
    #[serde(default)]
    needs:         Option<NeedEngine>,
    #[serde(default)]
    temporal_self: Option<TemporalSelf>,
    #[serde(default)]
    story:         Option<Story>,
    #[serde(default)]
    becoming:      Option<BecomingEngine>,
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
        needs:         None,
        temporal_self: None,
        story:         None,
        becoming:      None,
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

/// Save the KORE-BECOMING layer separately (called from heartbeat + tool calls)
pub fn save_becoming(
    owner:          &str,
    needs:          &NeedEngine,
    temporal:       &TemporalSelf,
    story:          &Story,
    becoming:       &BecomingEngine,
    tracker:        &EvolutionTracker,
    worldview:      &Worldview,
    narrative:      &NarrativeIdentity,
    values_engine:  &ValuesEngine,
    meaning:        &MeaningEngine,
    reality:        &RealityEngine,
    legacy:         &LegacyEngine,
    research:       &ResearchEngine,
    action_bridge:  &ActionBridge,
    goals:          &GoalEngine,
    federation:     &FederationEngine,
) -> std::io::Result<()> {
    let path = data_path(owner).with_file_name("becoming.kore.json");
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "needs":           needs,
        "temporal_self":   temporal,
        "story":           story,
        "becoming":        becoming,
        "evolution_tracker": tracker,
        "worldview":       worldview,
        "narrative":       narrative,
        "values_engine":   values_engine,
        "meaning":         meaning,
        "reality":         reality,
        "legacy":          legacy,
        "research":        research,
        "action_bridge":   action_bridge,
        "goals":           goals,
        "federation":      federation,
        "saved_at":        crate::now(),
    })).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json.as_bytes())?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Load KORE-BECOMING layer from disk (returns None if not saved yet).
/// Each sub-engine falls back to Default if missing (forward/backward compat).
pub fn load_becoming(owner: &str) -> Option<(
    NeedEngine, TemporalSelf, Story, BecomingEngine,
    EvolutionTracker, Worldview, NarrativeIdentity, ValuesEngine,
    MeaningEngine, RealityEngine, LegacyEngine, ResearchEngine,
    ActionBridge, GoalEngine, FederationEngine,
)> {
    let path = data_path(owner).with_file_name("becoming.kore.json");
    let bytes = fs::read(&path).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let needs:    NeedEngine      = serde_json::from_value(v["needs"].clone()).ok()?;
    let temporal: TemporalSelf    = serde_json::from_value(v["temporal_self"].clone()).ok()?;
    let story:    Story           = serde_json::from_value(v["story"].clone()).ok()?;
    let becoming: BecomingEngine  = serde_json::from_value(v["becoming"].clone()).ok()?;
    // These were added later — default if absent (old file compat)
    let tracker:  EvolutionTracker  = serde_json::from_value(v["evolution_tracker"].clone()).unwrap_or_default();
    let worldview: Worldview        = serde_json::from_value(v["worldview"].clone()).unwrap_or_default();
    let narrative: NarrativeIdentity= serde_json::from_value(v["narrative"].clone()).unwrap_or_default();
    let values_e:  ValuesEngine     = serde_json::from_value(v["values_engine"].clone()).unwrap_or_default();
    let meaning:   MeaningEngine    = serde_json::from_value(v["meaning"].clone()).unwrap_or_default();
    let reality:   RealityEngine    = serde_json::from_value(v["reality"].clone()).unwrap_or_default();
    let legacy:    LegacyEngine     = serde_json::from_value(v["legacy"].clone()).unwrap_or_default();
    let research:  ResearchEngine   = serde_json::from_value(v["research"].clone()).unwrap_or_default();
    let action_bridge: ActionBridge = serde_json::from_value(v["action_bridge"].clone()).unwrap_or_default();
    let goals: GoalEngine = serde_json::from_value(v["goals"].clone()).unwrap_or_default();
    let federation: FederationEngine = serde_json::from_value(v["federation"].clone()).unwrap_or_default();
    Some((needs, temporal, story, becoming, tracker, worldview, narrative, values_e, meaning, reality, legacy, research, action_bridge, goals, federation))
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
