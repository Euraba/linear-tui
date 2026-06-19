--- The native Neovim frontend: a dedicated tab with three panes — a sidebar
--- (Views / Teams / Projects), an issue list, and a detail pane — mirroring the
--- slack-tui layout the CLI uses. All data comes from the Rust backend over RPC
--- (see `rpc.lua`); this module is pure presentation + navigation.

local rpc = require("linear-tui.rpc")
local config = require("linear-tui.config")

local M = {}

-- Built-in client-side views, matching `models::View` order (My Issues first).
local VIEWS = {
  { key = "MyIssues", label = "My Issues" },
  { key = "Active", label = "Active" },
  { key = "Backlog", label = "Backlog" },
  { key = "All", label = "All" },
}

-- Sentinel "all projects" row at the top of the Projects pane (id = nil clears
-- the project filter).
local ALL_PROJECTS = { id = nil, name = "— All projects —" }

-- ----- formatting helpers --------------------------------------------------

local PRIORITY = { [1] = "URG", [2] = "HIGH", [3] = "MED", [4] = "LOW" }
local STATE_GLYPH = {
  backlog = "○",
  unstarted = "○",
  started = "◐",
  completed = "●",
  canceled = "✕",
  triage = "△",
}

local function priority_label(p)
  return PRIORITY[p] or "—"
end

local function state_glyph(st)
  return (st and STATE_GLYPH[st.type]) or "·"
end

local function state_name(st)
  return (st and st.name) or "—"
end

local function user_label(u)
  return u and (u.displayName or u.name) or nil
end

-- "2026-05-28T14:02:00.000Z" -> "2026-05-28 14:02"
local function fmt_date(s)
  if type(s) ~= "string" then
    return ""
  end
  local date, time = s:sub(1, 10), s:sub(12, 16)
  return time ~= "" and (date .. " " .. time) or date
end

local function notify(msg, level)
  vim.notify("linear-tui: " .. msg, level or vim.log.levels.INFO)
end

-- ----- state ---------------------------------------------------------------

local S

local function reset_state()
  S = {
    open = false,
    closing = false,
    win = {}, -- sidebar / list / detail window handles
    buf = {}, -- sidebar / list / detail buffer handles
    viewer = nil,
    teams = {},
    team_idx = 1,
    view_idx = 1,
    projects = { ALL_PROJECTS },
    project_idx = 1,
    issues = {},
    issue_idx = 0,
    detail = nil,
    sidebar_map = {}, -- buffer row -> { kind = "view"|"team"|"project", idx }
    list_map = {}, -- buffer row -> issue index
    states_cache = {}, -- team_id -> workflow states
    members_cache = {}, -- team_id -> members
    default_team = nil,
    issues_epoch = 0,
    detail_epoch = 0,
  }
end
reset_state()

-- Forward declarations (mutually-recursive locals).
local render_sidebar, render_list, render_detail
local reload_issues, open_detail, load_detail_by_id, load_projects, reload
local sidebar_select, setup_keymaps, setup_autocmds, build_layout, bootstrap
local change_state, change_assignee, add_comment, create_issue
local open_url, goto_parent, pick_child, show_help, cycle, focus

local function is_open()
  return S.open
    and S.win.sidebar
    and vim.api.nvim_win_is_valid(S.win.sidebar)
    and vim.api.nvim_win_is_valid(S.win.list)
    and vim.api.nvim_win_is_valid(S.win.detail)
end

local function set_lines(buf, lines)
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    return
  end
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
end

local function set_winbar(win, text)
  if win and vim.api.nvim_win_is_valid(win) then
    -- winbar is evaluated like 'statusline'; escape literal percent signs.
    vim.wo[win].winbar = text:gsub("%%", "%%%%")
  end
end

--- The team whose key prefixes an identifier (e.g. "ENG-128" -> ENG), so issues
--- reached via parent/sub-issue navigation use their own team for the
--- state/assignee pickers. Falls back to the selected sidebar team.
local function team_for_identifier(identifier)
  local key = identifier and identifier:match("^(%a+)%-")
  if key then
    for _, t in ipairs(S.teams) do
      if t.key == key then
        return t
      end
    end
  end
  return S.teams[S.team_idx]
end

local function require_detail()
  if not S.detail then
    notify("open an issue first (press <CR> in the list)", vim.log.levels.WARN)
    return nil
  end
  return S.detail
end

-- ----- rendering -----------------------------------------------------------

local function push(lines, map, text, entry)
  lines[#lines + 1] = text
  if entry then
    map[#lines] = entry
  end
end

function render_sidebar()
  local lines, map = {}, {}
  push(lines, map, " Views")
  for i, v in ipairs(VIEWS) do
    push(lines, map, ("  %s %s"):format(i == S.view_idx and "●" or " ", v.label), { kind = "view", idx = i })
  end
  push(lines, map, "")
  push(lines, map, " Teams")
  if #S.teams == 0 then
    push(lines, map, "   (loading…)")
  end
  for i, t in ipairs(S.teams) do
    local mark = i == S.team_idx and "▸" or " "
    push(lines, map, ("  %s %-4s %s"):format(mark, t.key or "", t.name or ""), { kind = "team", idx = i })
  end
  push(lines, map, "")
  push(lines, map, " Projects")
  for i, p in ipairs(S.projects) do
    local mark = i == S.project_idx and "▸" or " "
    push(lines, map, ("  %s %s"):format(mark, p.name or ""), { kind = "project", idx = i })
  end
  S.sidebar_map = map
  set_lines(S.buf.sidebar, lines)
  local who = user_label(S.viewer)
  set_winbar(S.win.sidebar, who and (" linear-tui · " .. who) or " linear-tui")
end

function render_list()
  local lines, map = {}, {}
  for i, it in ipairs(S.issues) do
    map[i] = i
    lines[i] = ("%s %-4s %-9s %s"):format(
      state_glyph(it.state),
      priority_label(it.priority),
      it.identifier or "",
      it.title or ""
    )
  end
  S.list_map = map
  set_lines(S.buf.list, #lines > 0 and lines or { "  (no issues)" })

  local team = S.teams[S.team_idx]
  local proj = S.projects[S.project_idx]
  local title = (" Issues · %s"):format(VIEWS[S.view_idx].label)
  if team then
    title = title .. ("  [%s]"):format(team.key or "")
  end
  if proj and proj.id then
    title = title .. "  /" .. (proj.name or "")
  end
  set_winbar(S.win.list, title .. ("  (%d)"):format(#S.issues))

  if #S.issues > 0 and vim.api.nvim_win_is_valid(S.win.list) then
    pcall(vim.api.nvim_win_set_cursor, S.win.list, { math.min(math.max(S.issue_idx, 1), #S.issues), 0 })
  end
end

function render_detail()
  local d = S.detail
  if not d then
    set_lines(S.buf.detail, { "", "  No issue selected." })
    set_winbar(S.win.detail, " Issue")
    return
  end
  local lines = {}
  local function add(s)
    lines[#lines + 1] = s
  end

  add("# " .. (d.identifier or "") .. "  " .. (d.title or ""))
  add(("state: %s   priority: %s   assignee: %s"):format(
    state_name(d.state),
    priority_label(d.priority),
    user_label(d.assignee) or "—"
  ))
  if d.url then
    add("url: " .. d.url)
  end
  if d.parent then
    add(("↑ parent: %s  %s"):format(d.parent.identifier or "", d.parent.title or ""))
  end
  add("")
  if d.description and d.description ~= "" then
    for _, l in ipairs(vim.split(d.description, "\n", { plain = true })) do
      add(l)
    end
  else
    add("_(no description)_")
  end

  if d.children and #d.children > 0 then
    add("")
    add(("── Sub-issues (%d) ──"):format(#d.children))
    for _, c in ipairs(d.children) do
      add(("  %s %-9s %s"):format(state_glyph(c.state), c.identifier or "", c.title or ""))
    end
  end

  if d.comments and #d.comments > 0 then
    add("")
    add(("── Comments (%d) ──"):format(#d.comments))
    for _, cm in ipairs(d.comments) do
      add(("%s  %s"):format(user_label(cm.user) or "?", fmt_date(cm.createdAt)))
      for _, l in ipairs(vim.split(cm.body or "", "\n", { plain = true })) do
        add("  " .. l)
      end
      add("")
    end
  end

  set_lines(S.buf.detail, lines)
  set_winbar(S.win.detail, " Issue · " .. (d.identifier or ""))
  if vim.api.nvim_win_is_valid(S.win.detail) then
    pcall(vim.api.nvim_win_set_cursor, S.win.detail, { 1, 0 })
  end
end

-- ----- data loading --------------------------------------------------------

function load_detail_by_id(issue_id)
  S.detail_epoch = S.detail_epoch + 1
  local epoch = S.detail_epoch
  set_winbar(S.win.detail, " Issue · loading…")
  set_lines(S.buf.detail, { "", "  loading…" })
  rpc.request("issue_detail", { issue_id = issue_id }, function(err, res)
    if not is_open() or epoch ~= S.detail_epoch then
      return
    end
    if err then
      set_lines(S.buf.detail, { "", "  error: " .. err })
      return
    end
    S.detail = res
    render_detail()
  end)
end

function open_detail(idx)
  local it = S.issues[idx]
  if not it then
    return
  end
  S.issue_idx = idx
  load_detail_by_id(it.id)
end

function reload_issues()
  local team = S.teams[S.team_idx]
  if not team then
    return
  end
  S.issues_epoch = S.issues_epoch + 1
  local epoch = S.issues_epoch
  set_lines(S.buf.list, { "  loading…" })
  local proj = S.projects[S.project_idx]
  rpc.request("issues", {
    team_id = team.id,
    view = VIEWS[S.view_idx].key,
    project_id = proj and proj.id or nil,
  }, function(err, res)
    if not is_open() or epoch ~= S.issues_epoch then
      return
    end
    if err then
      S.issues = {}
      set_lines(S.buf.list, { "  error: " .. err })
      return
    end
    S.issues = res or {}
    S.issue_idx = 0
    render_list()
    if #S.issues > 0 then
      open_detail(1)
    else
      S.detail = nil
      render_detail()
    end
  end)
end

function load_projects()
  local team = S.teams[S.team_idx]
  if not team then
    return
  end
  rpc.request("team_projects", { team_id = team.id }, function(err, res)
    if not is_open() then
      return
    end
    S.projects = { ALL_PROJECTS }
    if not err then
      for _, p in ipairs(res or {}) do
        S.projects[#S.projects + 1] = p
      end
    end
    S.project_idx = 1
    render_sidebar()
  end)
end

function reload()
  reload_issues()
  if S.detail then
    load_detail_by_id(S.detail.id)
  end
end

-- ----- navigation ----------------------------------------------------------

function sidebar_select()
  local row = vim.api.nvim_win_get_cursor(S.win.sidebar)[1]
  local e = S.sidebar_map[row]
  if not e then
    return
  end
  if e.kind == "view" then
    S.view_idx = e.idx
    render_sidebar()
    reload_issues()
  elseif e.kind == "team" then
    if e.idx ~= S.team_idx then
      S.team_idx = e.idx
      S.projects = { ALL_PROJECTS }
      S.project_idx = 1
      render_sidebar()
      load_projects()
      reload_issues()
    end
  elseif e.kind == "project" then
    S.project_idx = e.idx
    render_sidebar()
    reload_issues()
  end
end

function focus(win)
  if win and vim.api.nvim_win_is_valid(win) then
    vim.api.nvim_set_current_win(win)
  end
end

function cycle(dir)
  local order = { S.win.sidebar, S.win.list, S.win.detail }
  local cur = vim.api.nvim_get_current_win()
  for i, w in ipairs(order) do
    if w == cur then
      return focus(order[((i - 1 + dir) % 3) + 1])
    end
  end
  focus(S.win.list)
end

-- ----- actions (writes) ----------------------------------------------------

local function with_cache(cache, method, team_id, cb)
  if cache[team_id] then
    return cb(cache[team_id])
  end
  rpc.request(method, { team_id = team_id }, function(err, res)
    if err then
      return notify(err, vim.log.levels.ERROR)
    end
    cache[team_id] = res or {}
    cb(cache[team_id])
  end)
end

function change_state()
  local d = require_detail()
  if not d then
    return
  end
  local team = team_for_identifier(d.identifier)
  if not team then
    return
  end
  with_cache(S.states_cache, "team_states", team.id, function(states)
    if #states == 0 then
      return notify("no workflow states for this team", vim.log.levels.WARN)
    end
    vim.ui.select(states, {
      prompt = "Set state for " .. (d.identifier or ""),
      format_item = function(s)
        return s.name
      end,
    }, function(choice)
      if not choice then
        return
      end
      rpc.request("set_state", { issue_id = d.id, state_id = choice.id }, function(err)
        if err then
          return notify(err, vim.log.levels.ERROR)
        end
        notify("state → " .. choice.name)
        load_detail_by_id(d.id)
        reload_issues()
      end)
    end)
  end)
end

function change_assignee()
  local d = require_detail()
  if not d then
    return
  end
  local team = team_for_identifier(d.identifier)
  if not team then
    return
  end
  with_cache(S.members_cache, "team_members", team.id, function(members)
    local items = { { id = nil, label = "(Unassigned)" } }
    for _, m in ipairs(members) do
      items[#items + 1] = { id = m.id, label = user_label(m) }
    end
    vim.ui.select(items, {
      prompt = "Assign " .. (d.identifier or ""),
      format_item = function(x)
        return x.label
      end,
    }, function(choice)
      if not choice then
        return
      end
      local params = { issue_id = d.id }
      if choice.id then
        params.assignee_id = choice.id -- omitting the key unassigns
      end
      rpc.request("set_assignee", params, function(err)
        if err then
          return notify(err, vim.log.levels.ERROR)
        end
        notify("assignee → " .. choice.label)
        load_detail_by_id(d.id)
        reload_issues()
      end)
    end)
  end)
end

function add_comment()
  local d = require_detail()
  if not d then
    return
  end
  vim.ui.input({ prompt = "Comment on " .. (d.identifier or "") .. ": " }, function(body)
    if not body or body == "" then
      return
    end
    rpc.request("add_comment", { issue_id = d.id, body = body }, function(err)
      if err then
        return notify(err, vim.log.levels.ERROR)
      end
      notify("comment added")
      load_detail_by_id(d.id)
    end)
  end)
end

function create_issue()
  local team = S.teams[S.team_idx]
  if not team then
    return notify("no team selected", vim.log.levels.WARN)
  end
  vim.ui.input({ prompt = ("New issue in [%s]: "):format(team.key or "") }, function(title)
    if not title or title == "" then
      return
    end
    rpc.request("create_issue", { team_id = team.id, title = title }, function(err, res)
      if err then
        return notify(err, vim.log.levels.ERROR)
      end
      notify("created " .. ((res and res.identifier) or "issue"))
      reload_issues()
    end)
  end)
end

function open_url()
  local d = require_detail()
  if not d then
    return
  end
  if not d.url then
    return notify("issue has no url", vim.log.levels.WARN)
  end
  if vim.ui.open then
    vim.ui.open(d.url)
  else
    vim.fn.jobstart({ "xdg-open", d.url }, { detach = true })
  end
end

function goto_parent()
  local d = require_detail()
  if not d then
    return
  end
  if not d.parent then
    return notify("issue has no parent", vim.log.levels.WARN)
  end
  load_detail_by_id(d.parent.id)
end

function pick_child()
  local d = require_detail()
  if not d then
    return
  end
  if not (d.children and #d.children > 0) then
    return notify("issue has no sub-issues", vim.log.levels.WARN)
  end
  vim.ui.select(d.children, {
    prompt = "Open sub-issue",
    format_item = function(c)
      return (c.identifier or "") .. "  " .. (c.title or "")
    end,
  }, function(c)
    if c then
      load_detail_by_id(c.id)
    end
  end)
end

function show_help()
  notify(table.concat({
    "keys —",
    "  <Tab>/<S-Tab> cycle panes · q close · r reload",
    "  sidebar: <CR> select view/team/project",
    "  list: j/k hover-load · <CR> focus detail",
    "  actions: s state · a assignee · m comment · n new · o open-url · p parent · c sub-issue",
  }, "\n"))
end

-- ----- layout / lifecycle --------------------------------------------------

local function make_buf(ft)
  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = ft
  vim.bo[buf].modifiable = false
  return buf
end

local function map(buf, lhs, fn, desc)
  vim.keymap.set("n", lhs, fn, { buffer = buf, nowait = true, silent = true, desc = "linear-tui: " .. desc })
end

function setup_keymaps()
  local sb, ls, dt = S.buf.sidebar, S.buf.list, S.buf.detail
  for _, b in ipairs({ sb, ls, dt }) do
    map(b, "q", M.close, "close")
    map(b, "<Tab>", function()
      cycle(1)
    end, "next pane")
    map(b, "<S-Tab>", function()
      cycle(-1)
    end, "prev pane")
    map(b, "?", show_help, "help")
    map(b, "r", reload, "reload")
  end

  map(sb, "<CR>", sidebar_select, "select")

  map(ls, "<CR>", function()
    local row = vim.api.nvim_win_get_cursor(S.win.list)[1]
    if S.list_map[row] then
      open_detail(S.list_map[row])
      focus(S.win.detail)
    end
  end, "focus detail")

  for _, b in ipairs({ ls, dt }) do
    map(b, "s", change_state, "set state")
    map(b, "a", change_assignee, "set assignee")
    map(b, "m", add_comment, "comment")
    map(b, "n", create_issue, "new issue")
    map(b, "o", open_url, "open in browser")
    map(b, "p", goto_parent, "parent")
    map(b, "c", pick_child, "sub-issue")
  end
end

function setup_autocmds()
  local grp = vim.api.nvim_create_augroup("LinearTuiUi", { clear = true })
  -- Hovering a row in the list auto-loads its detail (epoch-guarded so fast
  -- scrolling discards superseded fetches). Programmatic cursor moves to the
  -- already-selected row are no-ops thanks to the idx check.
  vim.api.nvim_create_autocmd("CursorMoved", {
    group = grp,
    buffer = S.buf.list,
    callback = function()
      if not is_open() then
        return
      end
      local row = vim.api.nvim_win_get_cursor(S.win.list)[1]
      local idx = S.list_map[row]
      if idx and idx ~= S.issue_idx then
        open_detail(idx)
      end
    end,
  })
  -- If the user closes any of our windows, tear the whole UI down cleanly.
  vim.api.nvim_create_autocmd("WinClosed", {
    group = grp,
    callback = function(args)
      if S.closing or not S.open then
        return
      end
      local win = tonumber(args.match)
      if win == S.win.sidebar or win == S.win.list or win == S.win.detail then
        vim.schedule(M.close)
      end
    end,
  })
end

function build_layout()
  local cols = vim.o.columns
  local sidebar_w = config.opts.sidebar_width or 32
  local detail_w = config.opts.detail_width or math.max(48, math.min(72, math.floor(cols * 0.4)))

  vim.cmd("tabnew")
  S.win.detail = vim.api.nvim_get_current_win()
  vim.cmd("leftabove vsplit")
  S.win.list = vim.api.nvim_get_current_win()
  vim.cmd("leftabove vsplit")
  S.win.sidebar = vim.api.nvim_get_current_win()

  S.buf.sidebar = make_buf("linear-sidebar")
  S.buf.list = make_buf("linear-list")
  S.buf.detail = make_buf("markdown")
  vim.api.nvim_win_set_buf(S.win.sidebar, S.buf.sidebar)
  vim.api.nvim_win_set_buf(S.win.list, S.buf.list)
  vim.api.nvim_win_set_buf(S.win.detail, S.buf.detail)

  for _, w in ipairs({ S.win.sidebar, S.win.list }) do
    vim.wo[w].number = false
    vim.wo[w].relativenumber = false
    vim.wo[w].wrap = false
    vim.wo[w].cursorline = true
    vim.wo[w].signcolumn = "no"
    vim.wo[w].foldcolumn = "0"
    vim.wo[w].list = false
  end
  vim.wo[S.win.detail].number = false
  vim.wo[S.win.detail].wrap = true
  vim.wo[S.win.detail].linebreak = true
  vim.wo[S.win.detail].cursorline = false
  vim.wo[S.win.detail].signcolumn = "no"

  vim.api.nvim_win_set_width(S.win.sidebar, sidebar_w)
  vim.api.nvim_win_set_width(S.win.detail, detail_w)
  vim.wo[S.win.sidebar].winfixwidth = true
  vim.wo[S.win.detail].winfixwidth = true

  setup_keymaps()
  setup_autocmds()
end

function bootstrap()
  -- config -> teams -> projects -> issues (viewer is cosmetic, fetched alongside).
  rpc.request("config", nil, function(_, res)
    if not is_open() then
      return
    end
    S.default_team = config.opts.default_team or (res and res.default_team) or nil
    rpc.request("teams", nil, function(err, teams)
      if not is_open() then
        return
      end
      if err then
        set_lines(S.buf.list, { "  error: " .. err })
        return
      end
      S.teams = teams or {}
      S.team_idx = 1
      if S.default_team then
        for i, t in ipairs(S.teams) do
          if t.key == S.default_team then
            S.team_idx = i
            break
          end
        end
      end
      render_sidebar()
      load_projects()
      reload_issues()
    end)
  end)

  rpc.request("viewer", nil, function(err, v)
    if not err and v and is_open() then
      S.viewer = v
      render_sidebar()
    end
  end)
end

function M.open()
  if is_open() then
    return focus(S.win.list)
  end
  local bin, err = config.binary()
  if not bin then
    return notify(err, vim.log.levels.ERROR)
  end
  reset_state()
  S.open = true
  build_layout()
  render_sidebar()
  set_lines(S.buf.list, { "  loading…" })
  set_lines(S.buf.detail, { "" })
  focus(S.win.list)
  bootstrap()
end

function M.close()
  if S.closing then
    return
  end
  S.closing = true
  for _, w in pairs(S.win) do
    if w and vim.api.nvim_win_is_valid(w) then
      pcall(vim.api.nvim_win_close, w, true)
    end
  end
  pcall(vim.api.nvim_del_augroup_by_name, "LinearTuiUi")
  reset_state()
end

function M.toggle()
  if is_open() then
    M.close()
  else
    M.open()
  end
end

return M
