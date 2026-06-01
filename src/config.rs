//! Configuration is a Lua file that returns a table. We use Lua (rather than
//! JSON/TOML) deliberately: this TUI is meant to be portable into a Neovim
//! plugin later, where the same `config.lua` can be `require`d directly.
//!
//! Search order (first match wins):
//!   1. $LINEAR_TUI_CONFIG
//!   2. $XDG_CONFIG_HOME/linear-tui/config.lua
//!   3. ~/.config/linear-tui/config.lua
//!
//! Expected shape:
//! ```lua
//! return {
//!   api_key = "lin_api_xxx",      -- required (or set $LINEAR_API_KEY)
//!   default_team = "ENG",         -- optional team key to auto-select
//!   page_size = 50,               -- optional issue fetch limit
//! }
//! ```

use anyhow::{anyhow, Context, Result};
use mlua::{Lua, Value};
use std::path::PathBuf;

use crate::settings::CacheMode;

#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: String,
    pub default_team: Option<String>,
    pub page_size: u32,
    /// Initial cache mode (overridden by the saved state file / in-app Settings
    /// panel). `None` means "use the built-in default".
    pub cache_mode: Option<CacheMode>,
}

impl Config {
    /// Resolve the config path, honoring the env override and XDG layout.
    pub fn default_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("LINEAR_TUI_CONFIG") {
            return Some(PathBuf::from(p));
        }
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .ok()
            .or_else(dirs::config_dir)?;
        Some(base.join("linear-tui").join("config.lua"))
    }

    /// Load configuration from the default location. A missing file is only an
    /// error if we also can't find an API key in the environment.
    pub fn load() -> Result<Config> {
        let path = Self::default_path();
        let mut cfg = match &path {
            Some(p) if p.exists() => Self::from_lua_file(p)?,
            _ => Config {
                api_key: String::new(),
                default_team: None,
                page_size: 50,
                cache_mode: None,
            },
        };

        // Environment always wins for the secret, so it never has to touch disk.
        if let Ok(key) = std::env::var("LINEAR_API_KEY") {
            if !key.is_empty() {
                cfg.api_key = key;
            }
        }

        if cfg.api_key.is_empty() {
            let where_ = path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<no config dir>".into());
            return Err(anyhow!(
                "No Linear API key found.\n\
                 Set `api_key` in {where_} (a Lua file returning a table),\n\
                 or export LINEAR_API_KEY.\n\
                 Get a personal key at: Linear > Settings > Security & access > API."
            ));
        }
        Ok(cfg)
    }

    fn from_lua_file(path: &PathBuf) -> Result<Config> {
        let src = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Self::from_lua_str(&src)
            .with_context(|| format!("evaluating config {}", path.display()))
    }

    /// Evaluate a Lua chunk that returns a config table.
    pub fn from_lua_str(src: &str) -> Result<Config> {
        let lua = Lua::new();
        let value: Value = lua
            .load(src)
            .set_name("linear-tui config")
            .eval()
            .map_err(|e| anyhow!("Lua error: {e}"))?;

        let table = match value {
            Value::Table(t) => t,
            other => {
                return Err(anyhow!(
                    "config must `return` a table, got {}",
                    other.type_name()
                ))
            }
        };

        let api_key: String = table.get("api_key").unwrap_or_default();
        let default_team: Option<String> = match table.get::<Value>("default_team") {
            Ok(Value::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        let page_size: u32 = table.get("page_size").unwrap_or(50);
        let cache_mode = match table.get::<Value>("cache_mode") {
            Ok(Value::String(s)) => CacheMode::parse(&s.to_string_lossy()),
            _ => None,
        };

        Ok(Config {
            api_key,
            default_team,
            page_size: page_size.clamp(1, 250),
            cache_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_table() {
        let cfg = Config::from_lua_str(r#"return { api_key = "lin_abc" }"#).unwrap();
        assert_eq!(cfg.api_key, "lin_abc");
        assert_eq!(cfg.page_size, 50);
        assert!(cfg.default_team.is_none());
    }

    #[test]
    fn parses_full_table_and_allows_logic() {
        let cfg = Config::from_lua_str(
            r#"
            local key = os.getenv("NOPE") or "lin_fallback"
            return { api_key = key, default_team = "ENG", page_size = 25 }
            "#,
        )
        .unwrap();
        assert_eq!(cfg.api_key, "lin_fallback");
        assert_eq!(cfg.default_team.as_deref(), Some("ENG"));
        assert_eq!(cfg.page_size, 25);
    }

    #[test]
    fn rejects_non_table() {
        assert!(Config::from_lua_str("return 42").is_err());
    }

    #[test]
    fn parses_cache_mode_when_present() {
        let cfg =
            Config::from_lua_str(r#"return { api_key = "k", cache_mode = "memory" }"#).unwrap();
        assert_eq!(cfg.cache_mode, Some(CacheMode::Memory));
        // Absent / unknown values leave it unset (use the built-in default).
        let cfg = Config::from_lua_str(r#"return { api_key = "k" }"#).unwrap();
        assert_eq!(cfg.cache_mode, None);
        let cfg = Config::from_lua_str(r#"return { api_key = "k", cache_mode = "nope" }"#).unwrap();
        assert_eq!(cfg.cache_mode, None);
    }
}
