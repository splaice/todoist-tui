mod app;
mod model;
mod screen;
mod screenshot;
mod text;
mod theme;
mod todoist;
mod ui;
mod worker;

use std::io::{self, IsTerminal, Stdout, Write};
use std::sync::mpsc::{self, Sender};
use std::thread;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Effect, Job, Msg};
use crate::todoist::Client;

type Term = Terminal<CrosstermBackend<Stdout>>;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--screenshots") {
        let dir = args.next().unwrap_or_else(|| "screenshots".to_string());
        return screenshot::write_all(std::path::Path::new(&dir));
    }
    if !io::stdout().is_terminal() {
        eprintln!("tt needs an interactive terminal");
        std::process::exit(1);
    }

    enable_raw_mode()?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
    let _guard = Restore;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        Hide
    )?;

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = run(&mut terminal);
    drop(terminal);
    result
}

fn run(terminal: &mut Term) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let (mut app, jobs) = App::boot();
    spawn_jobs(&tx, app.client.clone(), jobs);

    loop {
        let mut drawn = screen::Screen::default();
        terminal.draw(|frame| {
            drawn = ui::draw(frame, &app);
        })?;
        app.adopt(drawn);

        if crossterm::event::poll(app.poll_timeout())? && handle_event(&mut app, &tx)? {
            break;
        }
        while let Ok(msg) = rx.try_recv() {
            let jobs = app.on_msg(msg);
            spawn_jobs(&tx, app.client.clone(), jobs);
        }
        app.tick();
    }
    Ok(())
}

fn handle_event(app: &mut App, tx: &Sender<Msg>) -> io::Result<bool> {
    let quit = match crossterm::event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Release => false,
        Event::Key(key) => {
            let effect = app.on_key(key);
            apply_effect(app, effect, tx)
        }
        Event::Mouse(mouse) => {
            let effect = app.on_mouse(mouse);
            apply_effect(app, effect, tx)
        }
        Event::Paste(text) => {
            let effect = app.on_paste(text);
            apply_effect(app, effect, tx)
        }
        Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => false,
    };
    Ok(quit)
}

fn apply_effect(app: &mut App, effect: Effect, tx: &Sender<Msg>) -> bool {
    match effect {
        Effect::None => false,
        Effect::Quit => true,
        Effect::Jobs(jobs) => {
            spawn_jobs(tx, app.client.clone(), jobs);
            false
        }
    }
}

fn spawn_jobs(tx: &Sender<Msg>, client: Option<Client>, jobs: Vec<Job>) {
    let Some(client) = client else {
        return;
    };
    for job in jobs {
        let tx = tx.clone();
        let client = client.clone();
        thread::spawn(move || {
            let _ = tx.send(worker::execute(&client, job));
        });
    }
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    let _ = out.flush();
}
