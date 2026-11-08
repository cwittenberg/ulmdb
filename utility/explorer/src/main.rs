mod app;
mod data;
mod editor;
mod files;
mod mutation;
mod text;
mod ui;

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event,
    },
    execute,
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("uexplorer: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut path = None;
    let mut emoji = true;
    let mut read_only = false;
    for argument in std::env::args_os().skip(1) {
        match argument.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "ulmdb explorer — visual database browser and record editor\n\nUsage: uexplorer [PATH] [--read-only] [--no-emoji]\n\nPATH may be an environment directory or one of its files.\nWithout PATH, browse from the current directory.\n\n↑↓ / j k  Move     Enter  Open / inspect     Tab  Switch pane\ne  Edit value     d / Delete  Confirm deletion     Shift+Delete  Delete now\nb  Hide/show databases     Ctrl+S  Save edit\n/  Search     r / F5  Refresh     o  Open another file     ?  Help     q  Quit\n\n--read-only  Disable editing and deletion.\n--no-emoji    Use plain labels for terminals without emoji fonts."
                );
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("uexplorer {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--no-emoji") => emoji = false,
            Some("--read-only") => read_only = true,
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option {value}; use --help").into());
            }
            _ if path.is_none() => path = Some(PathBuf::from(argument)),
            _ => return Err("only one starting path is accepted; use --help".into()),
        }
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "an interactive terminal is required; run uexplorer in a terminal window".into(),
        );
    }
    let explicit = path.is_some();
    let path = path.unwrap_or(std::env::current_dir()?);
    let mut app = app::App::new(&path, explicit, emoji)?;
    app.read_only = read_only;
    ratatui::run(|terminal| -> io::Result<()> {
        execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
        struct MouseGuard;
        impl Drop for MouseGuard {
            fn drop(&mut self) {
                let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
            }
        }
        let _mouse = MouseGuard;
        loop {
            app.receive();
            if app.dirty {
                terminal.draw(|frame| ui::draw(frame, &mut app))?;
                app.dirty = false;
            }
            if app.quit {
                break;
            }
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => app.key(key),
                    Event::Mouse(mouse) => app.mouse(mouse),
                    Event::Paste(text) => app.paste(&text),
                    Event::Resize(_, _) => app.dirty = true,
                    _ => {}
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}
