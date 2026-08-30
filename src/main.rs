use std::{io, sync::Arc, time::Duration};

use anyhow::Context;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use lazyswamp::{app::App, config::Config, swamp::SwampCli, ui};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    let client = Arc::new(SwampCli::new(
        config.swamp_bin.clone(),
        config.repo_dir.clone(),
    ));
    let mut app = App::new(config, client);
    let mut terminal = TerminalSession::new().context("could not initialize the terminal")?;
    app.begin_load();
    while !app.should_quit {
        terminal.terminal.draw(|frame| ui::render(frame, &app))?;
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
        {
            app.handle_key(key).await;
        }
        app.tick().await;
    }
    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}
