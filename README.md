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

## Install

| Platform        | Command                                                              |
| --------------- | -------------------------------------------------------------------- |
| Arch (`yay`)    | `yay -S linear-tui` (or `linear-tui-bin` for the prebuilt binary)    |
| Debian/Ubuntu   | download the `.deb` from [Releases], then `sudo apt install ./linear-tui_*_amd64.deb` |
| Prebuilt binary | grab the `*-x86_64-unknown-linux-gnu.tar.gz` from [Releases]         |
| From source     | `cargo install --git https://github.com/Euraba/linear-tui --locked`  |
| Build locally   | `cargo build --release` → `target/release/linear-tui`                |

See [docs/PACKAGING.md](docs/PACKAGING.md) for the full release/packaging flow.

[Releases]: https://github.com/Euraba/linear-tui/releases

## Setup

1. **Install** via one of the methods above (or `cargo build --release`).
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

| Key            | Action                                  |
| -------------- | --------------------------------------- |
| `h` / `l`      | move focus left / right between panes   |
| `Tab` / `S-Tab`| same (cycle pane focus)                 |
| `j`/`k`, ↑/↓   | move selection / scroll within a pane   |
| `Enter`        | focus the issue list / detail pane      |
| `/`            | find — jump between matches (`n`/`N`)    |
| `f`            | filter the issue list to matches (text) |
| `F`            | filter by assignee/creator/state/priority |
| `p`            | go to parent issue                      |
| `c`            | open a sub-issue (`⌫` to go back)       |
| `v`            | view embedded images (`n`/`p` cycle)    |
| `,`            | open settings (cache mode)              |
| `s`            | change issue state                      |
| `a`            | change assignee                         |
| `m`            | add a comment                           |
| `n`            | create a new issue (in current team)    |
| `N`            | create a sub-issue under the open issue |
| `r`            | reload current issue list               |
| `?`            | toggle help                             |
| `q` / `Ctrl-C` | quit                                    |

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

### `F` — server-side filter

While `/` and `f` filter the *loaded* list by text, **`F`** opens a filter editor
that queries Linear directly — so you can find issues that aren't in the current
view at all (e.g. *created by me*, *assigned to someone else*):

- **Assignee** — anyone, me, unassigned, or a specific person (`Enter` opens a
  person picker).
- **Creator** — anyone, me, or a specific person.
- **State** — any, or a workflow-state type (Todo / In Progress / Done / …).
- **Priority** — any, Urgent / High / Medium / Low / No priority.

`j`/`k` move between rows, `h`/`l` change a value (filters apply live), `Enter`
picks a person for the assignee/creator rows, `c` clears everything, `Esc`/`F`
closes. Active filters combine with the selected view/project and show in the
issues-pane title; an explicit assignee/state overrides the view's own (so
*My Issues* + *assignee: someone else* shows that person's issues).

## Sub-issues & parents

The detail pane shows an issue's relations: a `↑ parent` line when the issue is
a sub-issue, and a `── Sub-issues (N) ──` section listing its children.

- **`p`** jumps to the **parent** issue.
- **`c`** opens a **sub-issue picker**; choosing one opens it.
- **`⌫` (Backspace)** goes **back** to the previously-viewed issue.

Navigating this way works even when the parent/sub-issue isn't in the current
list or view — the detail pane shows the navigated issue (the list selection
stays put), and `s` / `a` / `m` act on whichever issue is on screen.

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

## CLI (`linear-tui <command>`)

The same binary is **also a scriptable CLI** — run with a subcommand and it does
one Linear operation and exits, instead of launching the TUI. It reuses the same
config, client and models, so there's no separate auth. Issues are addressed by
their human identifier (e.g. `ENG-123`); add `--json` to any command for
machine-readable output.

```bash
linear-tui me                         # the authenticated user
linear-tui teams                      # teams (KEY  Name)
linear-tui issues                     # "My Issues" across all teams (default)
linear-tui issues --team ENG --view active --limit 20
linear-tui search "redis timeout"     # full-text search across all issues
linear-tui view ENG-123               # full detail: description, sub-issues, comments
linear-tui states  --team ENG         # workflow states / projects / members
linear-tui projects --team ENG
linear-tui members  --team ENG

# writes (affect the real workspace)
linear-tui create --team ENG --title "Fix login" --priority 2 --assignee me
linear-tui create --team ENG --title "Sub-task" --parent ENG-123   # a sub-issue
linear-tui comment ENG-123 "shipping today"
linear-tui state   ENG-123 "In Progress"     # name, or done/todo/progress/backlog/canceled
linear-tui assign  ENG-123 me                # me | none | <name>

# no API key needed
linear-tui version                    # version + repository
linear-tui sponsor                    # how to fund the project / commercial support
```

`issues --view` is `my | active | backlog | all` (default `my`, cross-team).
Run `linear-tui --help` for the full list. This CLI is what makes the tool usable
by an agent (e.g. Claude Code) from any project — install the binary on your
`PATH` (`cargo build --release` then symlink `target/release/linear-tui` into a
`PATH` dir).

## Neovim plugin

This repo is **also a Neovim plugin** — the same binary, used as a backend. The
plugin is a thin native frontend (sidebar / issue list / detail panes in real
nvim windows); the Rust app runs headless as a co-process (`linear-tui serve`)
and does all the work — GraphQL, caching, and reading your API key. They talk
JSON-RPC over the child's stdin/stdout, so **the plugin never sees your token**
and there's no second copy of the API client to keep in sync.

```
Neovim (plugin) ──spawn──▶ linear-tui serve   (one child per nvim session)
    stdin  →  {"id":1,"method":"issues","params":{…}}
    stdout ←  {"id":1,"ok":true,"result":[ …issues… ]}
```

**Install** (any plugin manager pointed at this repo) — e.g. lazy.nvim:

```lua
{
  dir = "~/code/linear-tui",          -- or the repo URL
  build = "cargo build --release",     -- builds the backend binary
  opts = {},                           -- calls require("linear-tui").setup{}
  cmd = { "Linear", "LinearToggle", "LinearClose" },
}
```

Zero config is needed if `linear-tui` is on `$PATH` or built in this repo;
otherwise set `bin`. Configure your API key exactly as for the CLI (it's the
same binary): `api_key` in `config.lua` or `$LINEAR_API_KEY`.

| Command         | Action                          |
| --------------- | ------------------------------- |
| `:Linear`       | open the UI                     |
| `:LinearToggle` | toggle it                       |
| `:LinearClose`  | close it                        |

Inside: `<Tab>` cycles panes, `<CR>` in the sidebar switches view/team/project,
hovering an issue loads its detail, and `s`/`a`/`m`/`n`/`o`/`p`/`c` change
state / assignee / add a comment / create / open-in-browser / parent / sub-issue.
Run `:checkhealth linear-tui` to verify the binary is found and the backend can
authenticate. Full docs: `:help linear-tui`.

## Sponsor

linear-tui is MIT and stays that way — no paid tier, no licence key, nothing
behind a paywall, and it never phones home. If it's useful to you or your team,
sponsoring is what keeps it maintained: tracking Linear's API, triaging issues,
and shipping the apt/AUR packages.

**[Sponsor on GitHub →](https://github.com/sponsors/Euraba)** — tiers and what
each one buys are in [docs/SPONSORS.md](docs/SPONSORS.md).

Paid contract work (feature development, private packaging, integration work,
support retainers) is available separately: **rares@trydio.com**.

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
- **`serve.rs`** — `linear-tui serve`: a headless stdio JSON-RPC backend that
  reuses the same client/config/models to serve the Neovim plugin (see below).
- **`cli.rs`** — `linear-tui <command>`: a one-shot, scriptable CLI over the same
  client (for agents/scripts; see [CLI](#cli-linear-tui-command)).
- **`sponsor.rs`** — the funding links, in one place, rendered by the CLI, the
  `?` help overlay and the README (see [Sponsor](#sponsor)). No licence checks,
  no network calls.

Scope is intentionally "browse + core writes" (state, assignee, comment, create),
mirroring slack-tui's browse-plus-one-action shape.

[`mlua`]: https://crates.io/crates/mlua
[`ratatui`]: https://crates.io/crates/ratatui
[`ratatui-image`]: https://crates.io/crates/ratatui-image
[Kitty graphics protocol]: https://sw.kovidgoyal.net/kitty/graphics-protocol/
