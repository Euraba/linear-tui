//! User-modifiable runtime settings, surfaced in the in-app Settings overlay
//! (open with `,`) and persisted to a small JSON state file so they survive
//! restarts.
//!
//! This is deliberately separate from [`crate::config`] (the Lua config): we
//! never rewrite the hand-authored Lua file (it can contain arbitrary logic and
//! comments), so a runtime toggle is saved here instead. Startup precedence:
//! the state file wins, then the Lua `cache_mode` default, then the built-in.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How aggressively to cache fetched tickets (issue lists + issue details).
///
/// Caching is *stale-while-revalidate*: a cached copy is shown instantly and a
/// background refresh updates it, so cached data is never stale for long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheMode {
    /// No caching — every view is a fresh fetch.
    Off,
    /// Cache in memory: instant re-view of anything seen since launch.
    Memory,
    /// Cache in memory *and* on disk: also instant on the first view after a
    /// restart. Stored under the OS cache dir (`~/.cache/linear-tui`).
    #[default]
    Disk,
}

impl CacheMode {
    pub const ALL: [CacheMode; 3] = [CacheMode::Off, CacheMode::Memory, CacheMode::Disk];

    pub fn label(self) -> &'static str {
        match self {
            CacheMode::Off => "Off",
            CacheMode::Memory => "Memory",
            CacheMode::Disk => "Disk",
        }
    }

    /// A one-line description shown in the Settings overlay.
    pub fn describe(self) -> &'static str {
        match self {
            CacheMode::Off => "always fetch; never store",
            CacheMode::Memory => "instant re-view this session",
            CacheMode::Disk => "instant re-view, survives restart",
        }
    }

    /// Cycle Off → Memory → Disk → Off.
    pub fn cycle(self) -> CacheMode {
        match self {
            CacheMode::Off => CacheMode::Memory,
            CacheMode::Memory => CacheMode::Disk,
            CacheMode::Disk => CacheMode::Off,
        }
    }

    /// Parse a config string (case-insensitive); unknown values yield `None`.
    pub fn parse(s: &str) -> Option<CacheMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "false" => Some(CacheMode::Off),
            "memory" | "mem" | "ram" => Some(CacheMode::Memory),
            "disk" | "true" => Some(CacheMode::Disk),
            _ => None,
        }
    }

    /// Whether an in-memory cache is kept.
    pub fn uses_memory(self) -> bool {
        matches!(self, CacheMode::Memory | CacheMode::Disk)
    }

    /// Whether the cache is also persisted to disk.
    pub fn uses_disk(self) -> bool {
        matches!(self, CacheMode::Disk)
    }
}

/// The persisted runtime settings (currently just the cache mode).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub cache_mode: CacheMode,
}

impl Settings {
    /// Resolve effective settings: the saved state file wins, otherwise fall
    /// back to the Lua config's default, then the built-in default.
    pub fn load(config_default: Option<CacheMode>) -> Settings {
        if let Some(s) = Self::read() {
            return s;
        }
        Settings {
            cache_mode: config_default.unwrap_or_default(),
        }
    }

    fn read() -> Option<Settings> {
        let data = std::fs::read_to_string(state_path()?).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Persist to the state file (best-effort — failures are ignored).
    pub fn save(self) {
        let Some(path) = state_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// `~/.config/linear-tui/state.json` (honours `$XDG_CONFIG_HOME`), sitting next
/// to `config.lua`.
fn state_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(dirs::config_dir)?;
    Some(base.join("linear-tui").join("state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_visits_all_modes() {
        assert_eq!(CacheMode::Off.cycle(), CacheMode::Memory);
        assert_eq!(CacheMode::Memory.cycle(), CacheMode::Disk);
        assert_eq!(CacheMode::Disk.cycle(), CacheMode::Off);
    }

    #[test]
    fn parse_is_case_insensitive_and_strict() {
        assert_eq!(CacheMode::parse("Disk"), Some(CacheMode::Disk));
        assert_eq!(CacheMode::parse("  MEMORY "), Some(CacheMode::Memory));
        assert_eq!(CacheMode::parse("off"), Some(CacheMode::Off));
        assert_eq!(CacheMode::parse("wat"), None);
    }

    #[test]
    fn mode_capabilities() {
        assert!(!CacheMode::Off.uses_memory());
        assert!(CacheMode::Memory.uses_memory() && !CacheMode::Memory.uses_disk());
        assert!(CacheMode::Disk.uses_memory() && CacheMode::Disk.uses_disk());
    }

    #[test]
    fn default_is_disk() {
        assert_eq!(CacheMode::default(), CacheMode::Disk);
    }
}
