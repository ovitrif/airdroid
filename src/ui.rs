use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::style::{Attribute, Color, Print, SetAttribute, Stylize};
use crossterm::terminal;
use crossterm::terminal::{Clear, ClearType};

pub const CANCEL_HINT: &str = "Press either ⌃ + C, ESC, C or X to cancel and exit.";

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn status(message: impl AsRef<str>) {
    println!("{} {}", "◇".with(Color::Green), message.as_ref().bold());
}

pub fn success(message: impl AsRef<str>) {
    println!("{} {}", "✓".with(Color::Green), message.as_ref().bold());
}

pub fn warn(message: impl AsRef<str>) {
    println!(
        "{} {}",
        "!".with(Color::Yellow),
        format!("Warning: {}", message.as_ref()).with(Color::Yellow)
    );
}

pub fn error(message: impl AsRef<str>) {
    eprintln!(
        "\n{} {}",
        "×".with(Color::Red),
        format!("Error: {}", message.as_ref()).with(Color::Red)
    );
}

pub fn blank_line() {
    println!();
}

pub fn print_qr(qr: &str) {
    println!("{qr}");
}

pub fn title(name: &str, subtitle: &str) {
    println!(
        "{} {}",
        name.with(Color::Green).bold(),
        subtitle.with(Color::DarkGrey)
    );
}

pub fn section<I, S>(title: &str, lines: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    blank_line();
    println!("{} {}", "◇".with(Color::Green), title.bold());

    for line in lines {
        println!("{}  {}", "│".with(Color::DarkGrey), line.as_ref());
    }

    println!("{}", "└".with(Color::DarkGrey));
}

pub fn cancelled() -> anyhow::Error {
    Cancelled.into()
}

pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Cancelled>().is_some()
}

pub fn sleep_or_cancel(duration: Duration) -> Result<()> {
    if duration.is_zero() {
        return Ok(());
    }

    if terminal::enable_raw_mode().is_err() {
        thread::sleep(duration);
        return Ok(());
    }

    let raw_mode = RawModeGuard;
    let deadline = Instant::now() + duration;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());

        if remaining.is_zero() {
            return Ok(());
        }

        if !event::poll(remaining.min(Duration::from_millis(100)))
            .context("failed to poll keypress")?
        {
            continue;
        }

        match event::read().context("failed to read keypress")? {
            Event::Key(key)
                if key.kind == KeyEventKind::Press
                    && is_wait_cancel_key(key.code, key.modifiers) =>
            {
                drop(raw_mode);
                return Err(cancelled());
            }
            _ => {}
        }
    }
}

pub struct Countdown {
    label: String,
    last_seconds: Option<u64>,
    wrote_line: bool,
}

impl Countdown {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            last_seconds: None,
            wrote_line: false,
        }
    }

    pub fn tick(&mut self, remaining: Duration) -> Result<()> {
        let seconds = display_seconds(remaining);

        if self.last_seconds == Some(seconds) {
            return Ok(());
        }

        execute!(
            io::stdout(),
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            Print(format!(
                "{} {}: {seconds} seconds remaining...",
                "◇".with(Color::Green),
                self.label
            ))
        )?;
        io::stdout().flush().context("failed to flush stdout")?;

        self.last_seconds = Some(seconds);
        self.wrote_line = true;

        Ok(())
    }

    pub fn finish(&mut self) {
        if self.wrote_line {
            println!();
            self.wrote_line = false;
            self.last_seconds = None;
        }
    }
}

fn display_seconds(remaining: Duration) -> u64 {
    if remaining.is_zero() {
        0
    } else {
        remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
    }
}

pub fn menu(options: &[&str]) -> Result<usize> {
    if options.is_empty() {
        bail!("menu cannot be shown without options");
    }

    blank_line();
    status("Choose an action");

    if options.len() <= 9 {
        match interactive_menu(options) {
            Ok(value) => return Ok(value),
            Err(error) if is_raw_mode_error(&error) => {
                warn("interactive input is unavailable; press a number and Enter.");
            }
            Err(error) => return Err(error),
        }
    }

    line_menu(options)
}

fn interactive_menu(options: &[&str]) -> Result<usize> {
    terminal::enable_raw_mode().context("failed to enable raw terminal input")?;
    let raw_mode = RawModeGuard;
    let mut stdout = io::stdout();
    let mut selected = 0;

    render_interactive_menu(&mut stdout, options, selected)?;

    loop {
        match event::read().context("failed to read keypress")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    drop(raw_mode);
                    println!();
                    return Err(cancelled());
                }
                KeyCode::Esc => {
                    drop(raw_mode);
                    println!();
                    return Err(cancelled());
                }
                KeyCode::Up => {
                    selected = previous_selection(selected, options.len());
                    rerender_interactive_menu(&mut stdout, options, selected)?;
                }
                KeyCode::Down => {
                    selected = next_selection(selected, options.len());
                    rerender_interactive_menu(&mut stdout, options, selected)?;
                }
                KeyCode::Home => {
                    selected = 0;
                    rerender_interactive_menu(&mut stdout, options, selected)?;
                }
                KeyCode::End => {
                    selected = options.len() - 1;
                    rerender_interactive_menu(&mut stdout, options, selected)?;
                }
                KeyCode::Enter => {
                    let value = selected + 1;
                    let option = options[selected];
                    drop(raw_mode);
                    confirm_selection(&mut stdout, option)?;
                    return Ok(value);
                }
                KeyCode::Char(character) => {
                    if let Some(value) = selection_from_char(character, options.len()) {
                        let option = options[value - 1];
                        drop(raw_mode);
                        confirm_selection(&mut stdout, option)?;
                        return Ok(value);
                    }

                    print!("\x07");
                    io::stdout().flush().context("failed to flush stdout")?;
                }
                _ => {
                    print!("\x07");
                    io::stdout().flush().context("failed to flush stdout")?;
                }
            },
            _ => {}
        }
    }
}

fn confirm_selection<W: Write>(writer: &mut W, option: &str) -> Result<()> {
    execute!(
        writer,
        Clear(ClearType::CurrentLine),
        cursor::MoveToColumn(0),
        Print(format!("{} {option}\r\n", "✓".with(Color::Green)))
    )?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

fn render_interactive_menu<W: Write>(
    writer: &mut W,
    options: &[&str],
    selected: usize,
) -> Result<()> {
    for (index, option) in options.iter().enumerate() {
        execute!(writer, Clear(ClearType::CurrentLine))?;

        if index == selected {
            execute!(
                writer,
                SetAttribute(Attribute::Reverse),
                Print(format!("› {}. {option}", index + 1)),
                SetAttribute(Attribute::Reset),
                Print("\r\n")
            )?;
        } else {
            execute!(writer, Print(format!("  {}. {option}\r\n", index + 1)))?;
        }
    }

    execute!(
        writer,
        Clear(ClearType::CurrentLine),
        Print("Use ↑↓ + Enter, number key, or ESC to cancel: ")
    )?;
    writer.flush().context("failed to flush stdout")?;

    Ok(())
}

fn rerender_interactive_menu<W: Write>(
    writer: &mut W,
    options: &[&str],
    selected: usize,
) -> Result<()> {
    execute!(
        writer,
        cursor::MoveUp(options.len() as u16),
        cursor::MoveToColumn(0)
    )?;
    render_interactive_menu(writer, options, selected)
}

fn line_menu(options: &[&str]) -> Result<usize> {
    loop {
        print_options(options);
        let label = format!("Choose 1-{}", options.len());
        let input = prompt(&label)?;

        match input.trim().parse::<usize>() {
            Ok(value) if (1..=options.len()).contains(&value) => {
                success(options[value - 1]);
                return Ok(value);
            }
            _ => status("Please enter one of the numbered options."),
        }
    }
}

fn print_options(options: &[&str]) {
    for (index, option) in options.iter().enumerate() {
        println!("  {}. {option}", index + 1);
    }
}

fn selection_from_char(character: char, option_count: usize) -> Option<usize> {
    let value = character.to_digit(10)? as usize;

    if (1..=option_count).contains(&value) {
        Some(value)
    } else {
        None
    }
}

fn is_wait_cancel_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    match code {
        KeyCode::Esc => true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char(character) => matches!(character.to_ascii_lowercase(), 'c' | 'x'),
        _ => false,
    }
}

fn previous_selection(selected: usize, option_count: usize) -> usize {
    if selected == 0 {
        option_count - 1
    } else {
        selected - 1
    }
}

fn next_selection(selected: usize, option_count: usize) -> usize {
    (selected + 1) % option_count
}

fn is_raw_mode_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("raw terminal input"))
}

pub fn prompt(label: &str) -> Result<String> {
    print!("{} {label}: ", "?".with(Color::Green));
    io::stdout().flush().context("failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read input")?;

    Ok(input)
}

pub fn prompt_required(label: &str) -> Result<String> {
    loop {
        let input = prompt(label)?;
        let input = input.trim();

        if !input.is_empty() {
            return Ok(input.to_string());
        }

        status("Please enter a value.");
    }
}

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_digit_to_selection() {
        assert_eq!(selection_from_char('1', 2), Some(1));
        assert_eq!(selection_from_char('2', 2), Some(2));
        assert_eq!(selection_from_char('3', 2), None);
        assert_eq!(selection_from_char('x', 2), None);
    }

    #[test]
    fn recognizes_wait_cancel_keys() {
        assert!(is_wait_cancel_key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(is_wait_cancel_key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(is_wait_cancel_key(KeyCode::Char('C'), KeyModifiers::SHIFT));
        assert!(is_wait_cancel_key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ));
        assert!(is_wait_cancel_key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(is_wait_cancel_key(KeyCode::Char('X'), KeyModifiers::SHIFT));
        assert!(!is_wait_cancel_key(KeyCode::Char('q'), KeyModifiers::NONE));
    }

    #[test]
    fn rounds_countdown_seconds_up() {
        assert_eq!(display_seconds(Duration::ZERO), 0);
        assert_eq!(display_seconds(Duration::from_millis(1)), 1);
        assert_eq!(display_seconds(Duration::from_millis(1_001)), 2);
        assert_eq!(display_seconds(Duration::from_secs(2)), 2);
    }

    #[test]
    fn wraps_arrow_selection() {
        assert_eq!(previous_selection(0, 3), 2);
        assert_eq!(previous_selection(2, 3), 1);
        assert_eq!(next_selection(2, 3), 0);
        assert_eq!(next_selection(0, 3), 1);
    }
}
