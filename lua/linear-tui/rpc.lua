--- Thin JSON-RPC client over a long-lived `linear-tui serve` co-process.
---
--- One backend process is spawned per Neovim session (lazily, on first request)
--- and reused. Each request gets an incrementing id; responses arrive as
--- newline-delimited JSON on the child's stdout and are routed back to the
--- matching callback. Nothing here knows anything about Linear or your API key —
--- it just ships `{id, method, params}` lines and delivers `{id, ok, result}`.

local config = require("linear-tui.config")

local M = {}

local proc = nil ---@type vim.SystemObj|nil
local next_id = 0
local pending = {} ---@type table<integer, fun(err: string|nil, result: any)>
local stdout_acc = ""
local last_stderr = ""
local augroup = nil

--- Resolve every outstanding request with the same error (backend died / stopped).
local function fail_all(msg)
  local cbs = pending
  pending = {}
  for _, cb in pairs(cbs) do
    pcall(cb, msg)
  end
end

--- Handle one complete NDJSON line from the backend. Runs on the main loop.
local function handle_line(line)
  line = vim.trim(line)
  if line == "" then
    return
  end
  local ok, msg = pcall(vim.json.decode, line, { luanil = { object = true, array = true } })
  if not ok or type(msg) ~= "table" or msg.id == nil then
    return -- malformed or unsolicited; ignore
  end
  local cb = pending[msg.id]
  if not cb then
    return
  end
  pending[msg.id] = nil
  if msg.ok then
    cb(nil, msg.result)
  else
    cb(msg.error or "unknown backend error")
  end
end

-- stdout/stderr callbacks run in a |lua-loop-callbacks| (fast) context, so they
-- only buffer text and defer anything touching Neovim state via vim.schedule.
local function on_stdout(_, data)
  if not data then
    return
  end
  stdout_acc = stdout_acc .. data
  while true do
    local nl = stdout_acc:find("\n", 1, true)
    if not nl then
      break
    end
    local line = stdout_acc:sub(1, nl - 1)
    stdout_acc = stdout_acc:sub(nl + 1)
    vim.schedule(function()
      handle_line(line)
    end)
  end
end

local function on_stderr(_, data)
  if data and data ~= "" then
    last_stderr = (last_stderr .. data):sub(-2000)
  end
end

local function on_exit(obj)
  proc = nil
  stdout_acc = ""
  local detail = vim.trim(last_stderr ~= "" and last_stderr or (obj.stderr or ""))
  local msg = "linear-tui backend exited (code " .. tostring(obj.code) .. ")"
  if detail ~= "" then
    msg = msg .. ": " .. detail
  end
  vim.schedule(function()
    fail_all(msg)
  end)
end

--- Start the backend if it isn't running. Returns ok, err.
---@return boolean, string|nil
local function ensure_started()
  if proc then
    return true
  end
  local bin, err = config.binary()
  if not bin then
    return false, err
  end

  last_stderr, stdout_acc = "", ""
  local spawn_ok, handle = pcall(vim.system, { bin, "serve" }, {
    stdin = true,
    stdout = on_stdout,
    stderr = on_stderr,
    text = true,
  }, on_exit)
  if not spawn_ok then
    return false, "could not start backend: " .. tostring(handle)
  end
  proc = handle

  if not augroup then
    augroup = vim.api.nvim_create_augroup("LinearTuiRpc", { clear = true })
    vim.api.nvim_create_autocmd("VimLeavePre", {
      group = augroup,
      callback = function()
        M.stop()
      end,
    })
  end
  return true
end

--- Send a request. `cb(err, result)` is always called exactly once.
---@param method string
---@param params table|nil
---@param cb fun(err: string|nil, result: any)
function M.request(method, params, cb)
  cb = cb or function() end
  local ok, err = ensure_started()
  if not ok then
    return cb(err)
  end
  next_id = next_id + 1
  local id = next_id
  pending[id] = cb
  local line = vim.json.encode({ id = id, method = method, params = params or vim.empty_dict() })
  local wrote = pcall(function()
    proc:write(line .. "\n")
  end)
  if not wrote then
    pending[id] = nil
    cb("failed to write request to backend")
  end
end

--- Stop the backend (kills the child, fails any pending requests).
function M.stop()
  if proc then
    pcall(function()
      proc:kill("sigterm")
    end)
    proc = nil
  end
  fail_all("linear-tui backend stopped")
end

function M.is_running()
  return proc ~= nil
end

return M
