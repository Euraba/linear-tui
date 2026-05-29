# linear-tui

A terminal client for [Linear](https://linear.app), written in Rust. Modeled
after [slack-tui](https://github.com/hikalium/slack-tui): a narrow left column
of stacked list panes (Views / Teams / Projects), a wide issue list + detail
pane, and `Tab` to cycle focus. Defaults to your **My Issues** view on launch.

The official Linear app has no proper Linux build — this is a fast, keyboard-
driven alternative for browsing and triaging issues from the terminal.

```
┌─ Views ──────┐┌─ Issues · My Issues ────────────┐┌─ Issue ──────────────────┐
│ ● My Issues  ││ ◐ HIGH ENG-128 Fix login redirect ││ ENG-128 Fix login redirect │
│   Active     ││ ○ MED  ENG-131 Add dark mode      ││ state: In Progress         │
│   Backlog    ││ ◐ URG  ENG-140 Payment webhook... ││ assignee: rares            │
│   All        ││ ...                               ││ priority: High             │
├─ Teams ──────┤│                                   ││                            │
│ ENG  Eng     ││                                   ││ ── Comments (2) ──         │
├─ Projects ───┤│                                   ││ rares  2026-05-28 14:02    │
│ — All —      ││                                   ││   shipping this today      │
│ ▸ Q3 Launch  ││                                   ││                            │
└──────────────┘└───────────────────────────────────┘└────────────────────────────┘
 Signed in as rares   Enter:open  s:state  a:assign  m:comment  n:new  r:reload  ?:help
```

## Setup

1. **Build:** `cargo build --release` (binary at `target/release/linear-tui`).
2. **Configure:** copy `config.example.lua` to `~/.config/linear-tui/config.lua`
   and set your `api_key`. Get a personal key at
   *Linear → Settings → Security & access → Personal API keys*.

   The config is **Lua** (it must `return` a table) so it can later be shared
   with / `require`d from a Neovim plugin. You can run arbitrary Lua in it:

   ```lua
   return {
     api_key = os.getenv("LINEAR_API_KEY") or "lin_api_xxx",
     default_team = "ENG",
     page_size = 50,
   }
   ```

   Alternatively, skip the file entirely and just export `LINEAR_API_KEY`
   (the env var always overrides the config value).

3. **Run:** `linear-tui`.

Config is searched in this order: `$LINEAR_TUI_CONFIG`, then
`$XDG_CONFIG_HOME/linear-tui/config.lua`, then `~/.config/linear-tui/config.lua`.

## Keybindings

| Key            | Action                              |
| -------------- | ----------------------------------- |
| `Tab` / `S-Tab`| cycle pane focus                    |
| `j`/`k`, ↑/↓   | move selection / scroll detail      |
| `Enter`        | open team / view / issue            |
| `s`            | change issue state                  |
| `a`            | change assignee                     |
| `m`            | add a comment                       |
| `n`            | create a new issue (in current team)|
| `r`            | reload current issue list           |
| `?`            | toggle help                         |
| `q` / `Ctrl-C` | quit                                |

## Views, teams & projects

The left column is three live filter panes — moving the selection in any of them
reloads the issue list immediately (no `Enter`/`r` needed):

- **Views** (top): `My Issues` (default), `Active`, `Backlog`, `All` — built-in
  client-side filters, so the tool works for any team without depending on your
  saved Linear views.
- **Teams**: switching team also reloads its projects.
- **Projects**: narrows the issue list to one project. Combines with the current
  view (e.g. *My Issues in Project X*). The leading `— All projects —` row clears
  the project filter.

## Architecture

- **`config.rs`** — loads the Lua config via [`mlua`].
- **`client.rs`** — async GraphQL client over Linear's API (`reqwest`).
- **`worker.rs`** — a background Tokio task that owns the client; the UI talks
  to it over request/response channels so key presses never block on the network.
- **`app.rs`** — UI-side state + key handling (render-agnostic).
- **`ui.rs`** — all [`ratatui`] rendering.
- **`main.rs`** — terminal setup and the synchronous event loop.

Scope is intentionally "browse + core writes" (state, assignee, comment, create),
mirroring slack-tui's browse-plus-one-action shape.

[`mlua`]: https://crates.io/crates/mlua
[`ratatui`]: https://crates.io/crates/ratatui
