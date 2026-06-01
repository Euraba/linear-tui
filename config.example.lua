-- linear-tui configuration.
--
-- Copy to ~/.config/linear-tui/config.lua and fill in your key.
-- This file is plain Lua and must `return` a table — it's intentionally
-- Lua (not JSON/TOML) so it can later be `require`d directly from a Neovim
-- plugin sharing the same config.
--
-- Get a personal API key at:
--   Linear > Settings > Security & access > Personal API keys

return {
  -- Required. You can also leave this out and export LINEAR_API_KEY instead
  -- (the environment variable always overrides this value).
  api_key = "lin_api_xxxxxxxxxxxxxxxxxxxxxxxx",

  -- Optional: team key (e.g. "ENG") to highlight on startup.
  default_team = nil,

  -- Optional: how many issues to fetch per view (1–250).
  page_size = 50,

  -- Optional: initial ticket cache mode — "off", "memory", or "disk".
  -- Defaults to "disk". You can change this live from the in-app Settings
  -- panel (press ","); that choice is saved separately and takes precedence
  -- over this value on the next launch.
  cache_mode = "disk",
}
