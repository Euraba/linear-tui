--- linear-tui.nvim — a native Neovim frontend for the linear-tui Rust backend.
---
--- Architecture: the Rust binary runs as a headless co-process (`linear-tui
--- serve`) speaking JSON-RPC over stdio; this plugin is a thin frontend that
--- renders issues in native buffers and forwards actions. The backend owns the
--- API key, config, GraphQL and caching — the plugin never sees your token.
---
--- Quick start:
---   require("linear-tui").setup()   -- optional; zero-config works too
---   :Linear                          -- open the UI
---
--- See `:help linear-tui` for options and keymaps.

local M = {}

--- Configure the plugin (all fields optional).
---@param opts linear-tui.Opts|nil
function M.setup(opts)
  require("linear-tui.config").setup(opts)
end

--- Open the UI (no-op if already open; focuses it instead).
function M.open()
  require("linear-tui.ui").open()
end

--- Close the UI.
function M.close()
  require("linear-tui.ui").close()
end

--- Toggle the UI open/closed.
function M.toggle()
  require("linear-tui.ui").toggle()
end

--- Stop the backend co-process (it respawns on the next request).
function M.stop_backend()
  require("linear-tui.rpc").stop()
end

return M
