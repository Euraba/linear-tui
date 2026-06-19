//! linear-tui: a terminal client for Linear.app, modeled after slack-tui.
//!
//! The UI loop runs synchronously on the main thread; a Tokio runtime hosts a
//! single background worker that performs all network I/O. They communicate
//! over a pair of unbounded channels, so a key press never blocks on HTTP and
//! an in-flight request never blocks the UI.

mod app;
mod cache;
mod cli;
mod client;
mod config;
mod images;
mod models;
mod search;
mod serve;
mod settings;
mod ui;
mod worker;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc::unbounded_channel;

use app::App;
use client::LinearClient;
use config::Config;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() {
    // Routing by the first argument, none of which touch the terminal except
    // the default (no-args) interactive TUI:
    //   serve    -> stdio JSON-RPC backend for the Neovim plugin
    //   <cmd>    -> one-shot CLI (issues / view / create / …)
    //   (none)   -> interactive TUI
    let res = match std::env::args().nth(1).as_deref() {
        Some("serve") => serve::run(),
        Some(_) => cli::run(),
        None => run_tui(),
    };
    if let Err(e) = res {
        eprintln!("linear-tui: {}", config::redact_secrets(&format!("{e:#}")));
        std::process::exit(1);
    }
}

fn run_tui() -> Result<()> {
    // Resolve config (Lua file / env) before touching the terminal, so errors
    // print cleanly instead of inside the alternate screen.
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("linear-tui: {}", config::redact_secrets(&format!("{e:#}")));
            std::process::exit(1);
        }
    };

    let client = LinearClient::new(cfg.api_key.clone(), cfg.page_size)
        .context("building HTTP client")?;

    // Tokio runtime hosts the worker; the UI loop stays on the main thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting async runtime")?;

    let (tx_req, rx_req) = unbounded_channel();
    let (tx_resp, rx_resp) = unbounded_channel();

    {
        let _guard = runtime.enter();
        worker::spawn(client, rx_req, tx_resp);
    }

    let mut terminal = setup_terminal().context("setting up terminal")?;
    install_panic_hook();

    // Detect the terminal's graphics protocol + font size by querying stdio.
    // Must happen after raw mode is on (so we can read the reply). Kitty and
    // ghostty resolve to the Kitty protocol; anything else degrades to a
    // unicode-halfblocks renderer, which works everywhere.
    let picker = ratatui_image::picker::Picker::from_query_stdio()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks());

    // Effective settings: saved state file > Lua `cache_mode` > built-in default.
    let settings = settings::Settings::load(cfg.cache_mode);

    let mut app = App::new(tx_req, picker, settings.cache_mode);
    if let Some(team) = cfg.default_team {
        app.status = format!("Default team: {team} (select to load)");
    }
    app.bootstrap();

    let res = run(&mut terminal, &mut app, rx_resp);

    restore_terminal(&mut terminal).ok();
    // Dropping the runtime aborts the worker.
    drop(runtime);
    res
}

fn run(
    terminal: &mut Tui,
    app: &mut App,
    mut rx_resp: tokio::sync::mpsc::UnboundedReceiver<worker::Response>,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // Drain any worker responses without blocking.
        while let Ok(resp) = rx_resp.try_recv() {
            app.on_response(resp);
        }

        // Wait briefly for input; the timeout doubles as our redraw tick so the
        // spinner animates and responses surface promptly.
        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                // Ignore key-release/repeat noise (Windows / kitty protocol).
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Make sure a panic doesn't leave the user's terminal in raw/alt-screen mode.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));
}
