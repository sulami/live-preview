use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use tokio::{process, select, sync::mpsc};
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

#[tokio::main]
async fn main() -> Result<()> {
    console_subscriber::init();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    enable_raw_mode()?;

    let output = event_loop(&mut terminal).await?;

    execute!(stdout, LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()?;

    if let Some(o) = output {
        if !o.is_empty() {
            print!("{o}");
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
struct State {
    cursor: u16,
    input: String,
    output: String,
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<Option<String>> {
    let mut state = State::default();

    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>(1);
    let (output_tx, mut output_rx) = mpsc::channel::<String>(1);

    terminal.draw(|f| draw_ui(f, &state.cursor, &state.input, &state.output))?;

    tokio::spawn(child_handler(cmd_rx, output_tx));

    loop {
        select! {
            msg = output_rx.recv() => {
                if let Some(output) = msg {
                    state.output = output;
                    terminal.draw(|f| draw_ui(f, &state.cursor, &state.input, &state.output))?;
                }
            },
            msg = input_handler() => {
                if let Some(action) = msg {
                    match action {
                        Action::Done => {
                            cmd_tx.send(Cmd::Done).await?;
                            return Ok(Some(state.output))
                        },
                        Action::Abort => {
                            cmd_tx.send(Cmd::Done).await?;
                            return Ok(None)
                        },
                        Action::CursorLeft => if state.cursor > 0 {
                            state.cursor -= 1
                        },
                        Action::CursorRight => if state.cursor < state.input.len() as u16 {
                            state.cursor += 1
                        },
                        Action::Delete => if state.cursor > 0 {
                            state.cursor -= 1;
                            state.input.remove(state.cursor as usize);
                            cmd_tx.send(Cmd::Input(state.input.clone())).await?;
                        },
                        Action::Type(chr) => {
                            state.input.insert(state.cursor as usize, chr);
                            state.cursor += 1;
                            cmd_tx.send(Cmd::Input(state.input.clone())).await?;
                        }
                    }
                    terminal.draw(|f| draw_ui(f, &state.cursor, &state.input, &state.output))?;
                }
            },
        }
    }
}

#[derive(Eq, PartialEq, Clone, Copy)]
enum Action {
    Done,
    Abort,
    CursorLeft,
    CursorRight,
    Delete,
    Type(char),
}

async fn input_handler() -> Option<Action> {
    let mut event_stream = crossterm::event::EventStream::new();
    let action = match event_stream.next().await {
        // Abort
        Some(Ok(Event::Key(event::KeyEvent {
            code: KeyCode::Esc,
            kind: event::KeyEventKind::Press,
            ..
        }))) => Some(Action::Abort),
        // Done
        Some(Ok(Event::Key(event::KeyEvent {
            code: KeyCode::Enter,
            kind: event::KeyEventKind::Press,
            ..
        }))) => Some(Action::Done),
        // Cursor left
        Some(Ok(Event::Key(event::KeyEvent {
            code: KeyCode::Left,
            kind: event::KeyEventKind::Press,
            ..
        }))) => Some(Action::CursorLeft),
        // Cursor right
        Some(Ok(Event::Key(event::KeyEvent {
            code: KeyCode::Right,
            kind: event::KeyEventKind::Press,
            ..
        }))) => Some(Action::CursorRight),
        // Delete
        Some(Ok(Event::Key(event::KeyEvent {
            code: KeyCode::Backspace,
            kind: event::KeyEventKind::Press,
            ..
        }))) => Some(Action::Delete),
        // Typing
        Some(Ok(Event::Key(event::KeyEvent {
            code: KeyCode::Char(char),
            kind: event::KeyEventKind::Press,
            ..
        }))) => Some(Action::Type(char)),
        _ => None,
    };
    action
}

#[derive(Debug)]
enum Cmd {
    Input(String),
    Done,
}

async fn child_handler(
    mut cmd_chan: mpsc::Receiver<Cmd>,
    output_chan: mpsc::Sender<String>,
) -> Result<()> {
    let mut child_proc: Option<process::Child> = None;

    loop {
        select! {
            output = futures::future::OptionFuture::from(child_proc.map(|c| c.wait_with_output())) => {
                if let Some(Ok(o)) = output {
                    if !o.stdout.is_empty() {
                        if let Ok(s) = String::from_utf8(o.stdout) {
                            output_chan.send(s).await?;
                        } else {
                            output_chan.send("".to_string()).await?;
                        }
                    } else if !o.stderr.is_empty() {
                        if let Ok(s) = String::from_utf8(o.stderr) {
                            output_chan.send(s).await?;
                        } else {
                            output_chan.send("".to_string()).await?;
                        }
                    } else {
                        output_chan.send("".to_string()).await?;
                    }
                }
                child_proc = None;
            },
            msg = cmd_chan.recv() => {
                if let Some(cmd) = msg {
                    match cmd {
                        Cmd::Input(input) => {
                            let proc = process::Command::new("zsh")
                                .arg("-c")
                                .arg(&input)
                                .stdin(std::process::Stdio::piped())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::piped())
                                .kill_on_drop(true)
                                .spawn();
                            if let Ok(p) = proc {
                                child_proc = Some(p);
                            } else {
                                child_proc = None;
                            }
                        },
                        Cmd::Done => return Ok(()),
                    }
                } else {
                    child_proc = None;
                }
            },
        }
    }
}

fn draw_ui(
    f: &mut Frame<CrosstermBackend<std::io::Stdout>>,
    cursor: &u16,
    input: &str,
    output: &str,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(3), Constraint::Min(3)].as_ref())
        .split(f.size());

    // TODO Add dynamic resize.
    let input_box = Paragraph::new(input)
        .block(Block::default().title("Stdin").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    f.render_widget(input_box, chunks[0]);

    // TODO Add vertical scrolling.
    let output_box =
        Paragraph::new(output).block(Block::default().title("Stdout").borders(Borders::ALL));
    f.render_widget(output_box, chunks[1]);

    f.set_cursor(2 + cursor, 2);
}
