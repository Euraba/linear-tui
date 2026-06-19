--- `:checkhealth linear-tui` — verifies the plugin can find and talk to the
--- Rust backend, and that the backend can authenticate to Linear.

local M = {}

function M.check()
  local health = vim.health
  health.start("linear-tui")

  -- Neovim version (vim.system / stdio co-process needs 0.10+).
  if vim.fn.has("nvim-0.10") == 1 then
    local v = vim.version()
    health.ok(("Neovim %d.%d.%d"):format(v.major, v.minor, v.patch))
  else
    health.error("Neovim 0.10+ required (uses vim.system for the backend co-process)")
    return
  end

  -- Locate the backend binary.
  local config = require("linear-tui.config")
  local bin, err = config.binary()
  if not bin then
    health.error(err or "linear-tui binary not found")
    return
  end
  health.ok("backend binary: " .. bin)

  -- Live round-trip: spawn `serve`, send one `viewer` request, read the reply.
  -- Passing the request as stdin means the child gets EOF and exits cleanly
  -- after responding, so :wait() returns promptly.
  local req = vim.json.encode({ id = 1, method = "viewer", params = vim.empty_dict() }) .. "\n"
  local res = vim.system({ bin, "serve" }, { stdin = req, text = true }):wait(15000)

  if res.code ~= 0 and (res.stdout == nil or res.stdout == "") then
    health.error("backend exited (code " .. tostring(res.code) .. ")", vim.trim(res.stderr or ""))
    return
  end

  local line = vim.split(vim.trim(res.stdout or ""), "\n", { plain = true })[1] or ""
  local ok, msg = pcall(vim.json.decode, line, { luanil = { object = true, array = true } })
  if not ok or type(msg) ~= "table" then
    health.error("could not parse backend response", line)
    return
  end
  if msg.ok and msg.result then
    local name = msg.result.displayName or msg.result.name or "?"
    health.ok("authenticated to Linear as " .. name)
  else
    health.error("backend could not authenticate: " .. tostring(msg.error))
    health.info("Set api_key in your config.lua or export LINEAR_API_KEY (the CLI reads the same config).")
  end
end

return M
