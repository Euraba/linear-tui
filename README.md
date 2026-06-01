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
| `Enter`        | focus the issue list / detail pane  |
| `/`            | find — jump between matches (`n`/`N`)|
| `f`            | filter the issue list to matches    |
| `v`            | view embedded images (`n`/`p` cycle)|
| `,`            | open settings (cache mode)          |
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

Likewise, **hovering an issue** in the issue list loads its description and
comments into the detail pane automatically — no `Enter` needed. `Enter` just
moves focus into the detail pane so you can scroll long issues.

## Search & filter

Two ripgrep-flavoured tools, both **smart-case** (case-insensitive unless your
query has an uppercase letter):

- **`/` — find / jump.** In the **issue list** it jumps the selection between
  matching rows; in the **open issue** it highlights matches in the text and
  scrolls between them. Type the query and press `Enter`; then `n` / `N` cycle
  forward / back through matches. `Esc` clears it.
- **`f` — filter.** Narrows the issue list to matching issues (the pane title
  shows `(visible/total)`), like ripgrep showing only matching lines. The list
  filters live as you type; `Enter` keeps it, `Esc` clears it.

Both match across an issue's identifier, title, assignee and state. The filter
persists as you switch views/teams (press `Esc` in the list to drop it).

## Images

Linear issues often embed screenshots (markdown `![](…)` in the description and
comments). When the hovered issue has images, the detail pane shows a
`🖼 N image(s) · press v` hint; pressing `v` opens a full-pane viewer. Inside it,
`n`/`p` (or arrows / `j`·`k`) cycle through the images and `Esc`/`q` closes.

Images are rendered in-terminal via the [Kitty graphics protocol], so they show
up as real pictures in **kitty** and **ghostty**. Other terminals fall back
automatically to a unicode-halfblocks approximation (lower fidelity, but works
everywhere). Linear-hosted images are fetched with your API key; the key is
**never** sent to third-party image hosts referenced in an issue.

Downloaded images are cached on disk under `~/.cache/linear-tui/images/`
(honouring `$XDG_CACHE_HOME`), keyed by a hash of the URL, so they aren't
re-downloaded on subsequent runs. Delete that directory to clear the cache.

## Caching tickets

Fetching an issue list or an issue's detail is a network round-trip (~1s). To
avoid waiting on every view, the tool caches tickets and uses
**stale-while-revalidate**: the cached copy is shown *instantly*, and a refresh
runs in the background so what you see is never stale for long (states,
comments and assignees still update within a moment of viewing).

Press `,` to open the **Settings** panel and cycle the cache mode with
`←`/`→` (or `Enter`):

| Mode     | Behaviour                                                        |
| -------- | ---------------------------------------------------------------- |
| `Off`    | no caching — every view is a fresh fetch                         |
| `Memory` | instant re-view of anything seen since launch (cleared on quit)  |
| `Disk`   | as Memory, **and** instant on the first view after a restart     |

`Disk` (the default) persists ticket JSON under `~/.cache/linear-tui/tickets/`.
The selected mode is saved to `~/.config/linear-tui/state.json` and restored on
the next launch; you can also set the startup default in `config.lua` with
`cache_mode = "off" | "memory" | "disk"` (the saved panel choice wins over it).
Delete `~/.cache/linear-tui/` to clear the cache.

## Architecture

- **`config.rs`** — loads the Lua config via [`mlua`].
- **`client.rs`** — async GraphQL client over Linear's API (`reqwest`), plus
  image downloads.
- **`worker.rs`** — a background Tokio task that owns the client; the UI talks
  to it over request/response channels so key presses never block on the network.
  Image fetches run as detached tasks so a slow download never stalls navigation.
- **`images.rs`** — image-URL extraction from markdown, the disk-cache path, and
  the per-URL render state.
- **`cache.rs`** — on-disk ticket cache (issue lists + details) for `Disk` mode.
- **`settings.rs`** — the runtime cache-mode setting + its `state.json` store.
- **`search.rs`** — smart-case substring matching for `/` find and `f` filter.
- **`app.rs`** — UI-side state + key handling (render-agnostic).
- **`ui.rs`** — all [`ratatui`] rendering ([`ratatui-image`] for the viewer).
- **`main.rs`** — terminal setup and the synchronous event loop.

Scope is intentionally "browse + core writes" (state, assignee, comment, create),
mirroring slack-tui's browse-plus-one-action shape.

[`mlua`]: https://crates.io/crates/mlua
[`ratatui`]: https://crates.io/crates/ratatui
[`ratatui-image`]: https://crates.io/crates/ratatui-image
[Kitty graphics protocol]: https://sw.kovidgoyal.net/kitty/graphics-protocol/
