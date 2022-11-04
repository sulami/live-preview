use std::io;
use std::process::{Command, Stdio};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

fn main() -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    enable_raw_mode()?;

    let output = handle_input(&mut terminal)?;

    execute!(stdout, LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()?;

    if let Some(o) = output {
        if !o.is_empty() {
            print!("{}", o);
        }
    }

    Ok(())
}

fn handle_input(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<Option<String>> {
    let mut input = String::new();
    let mut output = String::new();
    let mut errors = String::new();
    let mut cursor = 0;

    terminal.draw(|f| draw_ui(f, &cursor, &input, &output, &errors))?;

    loop {
        match event::read()? {
            // Abort
            Event::Key(event::KeyEvent {
                code: KeyCode::Esc,
                kind: event::KeyEventKind::Press,
                ..
            }) => return Ok(None),
            // Done
            Event::Key(event::KeyEvent {
                code: KeyCode::Enter,
                kind: event::KeyEventKind::Press,
                ..
            }) => {
                return Ok(Some(output));
            }
            // Cursor left
            Event::Key(event::KeyEvent {
                code: KeyCode::Left,
                kind: event::KeyEventKind::Press,
                ..
            }) => {
                if cursor > 0 {
                    cursor -= 1;
                }
            }
            // Cursor right
            Event::Key(event::KeyEvent {
                code: KeyCode::Right,
                kind: event::KeyEventKind::Press,
                ..
            }) => {
                if cursor < input.len() as u16 {
                    cursor += 1
                }
            }
            // Delete
            Event::Key(event::KeyEvent {
                code: KeyCode::Backspace,
                kind: event::KeyEventKind::Press,
                ..
            }) => {
                if cursor > 0 {
                    cursor -= 1;
                    input.remove(cursor as usize);
                }
            }
            // Typing
            Event::Key(event::KeyEvent {
                code: KeyCode::Char(char),
                kind: event::KeyEventKind::Press,
                ..
            }) => {
                input.insert(cursor as usize, char);
                cursor += 1;
            }
            _ => (),
        }
        let split_input: Vec<&str> = input.split_whitespace().collect();
        if let Some(c) = split_input.first() {
            let proc = Command::new(c)
                .args(split_input.iter().skip(1))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            if let Ok(p) = proc {
                let result = p.wait_with_output().expect("Failed to wait for command");
                errors = String::from_utf8(result.stderr).expect("Command stderr is not utf-8");
                output = String::from_utf8(result.stdout).expect("Command stdout is not utf-8");
            } else {
                errors.clear();
                output.clear();
            }
        } else {
            errors.clear();
            output.clear();
        }
        terminal.draw(|f| draw_ui(f, &cursor, &input, &output, &errors))?;
    }
}

fn draw_ui(
    f: &mut Frame<CrosstermBackend<std::io::Stdout>>,
    cursor: &u16,
    input: &str,
    output: &str,
    errors: &str,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(3), Constraint::Min(3)].as_ref())
        .split(f.size());

    // TODO Add dynamic resize.
    let input_box = Paragraph::new(input)
        .block(Block::default().title("Input").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    f.render_widget(input_box, chunks[0]);

    if !errors.is_empty() {
        let errors_box =
            Paragraph::new(errors).block(Block::default().title("Stderr").borders(Borders::ALL));
        f.render_widget(errors_box, chunks[1]);
    }

    // TODO Add vertical scrolling.
    let output_box =
        Paragraph::new(output).block(Block::default().title("Stdout").borders(Borders::ALL));
    f.render_widget(output_box, chunks[1]);

    f.set_cursor(2 + cursor, 2);
}
