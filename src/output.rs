//! Terminal presentation shared by the commands and tool wrappers.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::text::{fit, strip_ansi};
use crate::{env, process, progress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    CyanBold,
    GreenBold,
    RedBold,
    Green,
    Red,
    Yellow,
    YellowBold,
    Dimmed,
    Bold,
}

impl Style {
    pub fn code(self) -> &'static str {
        match self {
            Style::CyanBold => "\u{1b}[1;36m",
            Style::GreenBold => "\u{1b}[1;32m",
            Style::RedBold => "\u{1b}[1;31m",
            Style::Green => "\u{1b}[32m",
            Style::Red => "\u{1b}[31m",
            Style::Yellow => "\u{1b}[33m",
            Style::YellowBold => "\u{1b}[1;33m",
            Style::Dimmed => "\u{1b}[2m",
            Style::Bold => "\u{1b}[1m",
        }
    }
}

pub const RESET: &str = "\u{1b}[0m";
/// Returns to the first reserved progress row and erases both.
pub const CLEARED: &str = "\u{1b}[2A\r\u{1b}[2K\u{1b}[1B\r\u{1b}[2K\u{1b}[1A\r";

/// What a quiet run printed, kept for its result instead of the terminal.
#[derive(Debug, Default)]
pub struct Captured {
    pub stdout: String,
    pub stderr: String,
}

struct Screen {
    captured: Option<Captured>,
    phase: usize,
    phases: usize,
}

static SCREEN: Mutex<Screen> = Mutex::new(Screen {
    captured: None,
    phase: 0,
    phases: 1,
});

fn screen() -> std::sync::MutexGuard<'static, Screen> {
    SCREEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Starts keeping output instead of printing it.
pub fn capture() {
    screen().captured = Some(Captured::default());
}

/// Stops keeping output and returns what was kept.
pub fn release() -> Captured {
    screen().captured.take().unwrap_or_default()
}

fn capturing() -> bool {
    screen().captured.is_some()
}

fn plain_requested() -> bool {
    env::is_set(env::NO_COLOR) || env::text(env::CLICOLOR).as_deref() == Some("0")
}

fn capable_terminal() -> bool {
    std::io::stdout().is_terminal() && env::text(env::TERM).as_deref() != Some("dumb")
}

fn colored() -> bool {
    if capturing() || plain_requested() {
        return false;
    }
    match env::text(env::CLICOLOR_FORCE).as_deref() {
        Some(force) if !force.is_empty() && force != "0" => true,
        _ => capable_terminal(),
    }
}

/// Whether progress may redraw rows in place.
fn live() -> bool {
    !capturing() && !plain_requested() && capable_terminal()
}

pub fn styled(text: &str, style: Style) -> String {
    if colored() {
        format!("{}{text}{RESET}", style.code())
    } else {
        text.to_string()
    }
}

fn emit(stderr: bool, text: &str) {
    let mut screen = screen();
    if let Some(captured) = screen.captured.as_mut() {
        let target = if stderr {
            &mut captured.stderr
        } else {
            &mut captured.stdout
        };
        target.push_str(text);
        return;
    }
    drop(screen);
    if stderr {
        let _ = std::io::stderr().write_all(text.as_bytes());
    } else {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    }
}

pub fn line(text: &str) {
    emit(false, &format!("{text}\n"));
}

pub fn line_err(text: &str) {
    emit(true, &format!("{text}\n"));
}

/// The width of the label column that facts share.
pub const LABEL: usize = 10;

/// How many characters of a revision are shown.
pub const REVISION: usize = 12;

/// The part of a revision that is shown.
pub fn revision(revision: &str) -> String {
    revision.chars().take(REVISION).collect()
}

/// The width of a label column that holds these names: `LABEL`, or the
/// longest name and two more.
pub fn widened<'a>(names: impl IntoIterator<Item = &'a str>) -> usize {
    names
        .into_iter()
        .map(|name| name.chars().count() + 2)
        .max()
        .unwrap_or(0)
        .max(LABEL)
}

/// What a row reports. The mark and its colour carry the state, so that the
/// hierarchy also reads without colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Done,
    Failed,
    Attention,
    /// Nothing happened: unchanged or skipped.
    Quiet,
}

impl Mark {
    fn symbol(self) -> &'static str {
        match self {
            Mark::Done => "✓",
            Mark::Failed => "✗",
            Mark::Attention => "!",
            Mark::Quiet => "·",
        }
    }

    fn style(self) -> Style {
        match self {
            Mark::Done => Style::Green,
            Mark::Failed => Style::Red,
            Mark::Attention => Style::Yellow,
            Mark::Quiet => Style::Dimmed,
        }
    }

    pub fn styled(self) -> String {
        styled(self.symbol(), self.style())
    }
}

pub fn dim(text: &str) -> String {
    styled(text, Style::Dimmed)
}

fn indented(stderr: bool, indent: usize, text: &str) {
    emit(stderr, &format!("{:indent$}{text}\n", ""));
}

/// The context of a run: where and on what it works. Supporting text.
pub fn context(text: &str) {
    line(&dim(text));
}

/// A section of a report: a bold heading with a muted note.
pub fn section(text: &str, note: &str) {
    line("");
    let note = if note.is_empty() {
        String::new()
    } else {
        dim(&format!(" · {note}"))
    };
    line(&format!("{}{note}", styled(text, Style::Bold)));
}

/// A fact: a label in the shared column and its value.
pub fn fact(label: &str, value: &str) {
    fact_in(LABEL, label, value);
}

/// A fact whose label column is as wide as given, such as `widened` says.
pub fn fact_in(width: usize, label: &str, value: &str) {
    indented(false, 2, &format!("{label:<width$}{value}"));
}

/// A row with its state: what happened to one thing.
pub fn row(mark: Mark, text: &str) {
    let text = if mark == Mark::Quiet {
        dim(text)
    } else {
        text.to_string()
    };
    indented(false, 2, &format!("{} {text}", mark.styled()));
}

/// A supporting line beneath a row, in the column of its text.
pub fn beneath(text: &str) {
    indented(false, 4 + LABEL, &dim(text));
}

/// A supporting fact beneath a row.
pub fn sub(label: &str, value: &str) {
    indented(false, 4, &dim(&format!("{label:<LABEL$}{value}")));
}

/// A statement about the outcome, in plain weight.
pub fn effect(text: &str) {
    indented(false, 2, text);
}

pub fn effect_err(text: &str) {
    indented(true, 2, text);
}

/// Supporting text beneath an outcome.
pub fn note(text: &str) {
    indented(false, 2, &dim(text));
}

pub fn note_err(text: &str) {
    indented(true, 2, &dim(text));
}

/// A command to run, set apart by weight.
pub fn command(text: &str) {
    indented(false, 4, &styled(text, Style::Bold));
}

pub fn command_err(text: &str) {
    indented(true, 4, &styled(text, Style::Bold));
}

/// Temporary rows of progress and streamed tool output.
pub fn detail(text: &str) {
    for row in text.lines() {
        indented(false, 4, &dim(row));
    }
}

/// Sets how many phases already ran and how many the run has in total.
pub fn phases(done: usize, total: usize) {
    let mut screen = screen();
    screen.phase = done;
    screen.phases = total;
}

/// A phase of work: the active heading of an append-only log.
pub fn start(text: &str) {
    let prefix = {
        let mut screen = screen();
        screen.phase += 1;
        if screen.phases > 1 {
            format!("[{}/{}] ", screen.phase, screen.phases)
        } else {
            String::new()
        }
    };
    line("");
    line(&styled(&format!("{prefix}{text}"), Style::CyanBold));
}

/// Shows what is in progress until `settle` replaces it. On a terminal only:
/// an append-only log gets the finished row instead.
pub fn pending(text: &str) -> bool {
    if !live() {
        return false;
    }
    // Clipped, so that the one row can be erased again.
    let width = columns().saturating_sub(1).max(1);
    emit(
        false,
        &format!("{}\n", dim(&fit(&format!("  … {text}"), width))),
    );
    true
}

/// Erases the row `pending` printed.
pub fn settle(pending: bool) {
    if pending {
        emit(false, "\u{1b}[1A\r\u{1b}[2K");
    }
}

/// The one outcome of a successful run.
pub fn success(text: &str) {
    line("");
    line(&styled(&format!("✓ {text}"), Style::GreenBold));
}

/// The one outcome of a run that worked but found something to attend to.
pub fn caution(text: &str) {
    line("");
    line(&styled(&format!("! {text}"), Style::YellowBold));
}

/// The one outcome of a failed run.
pub fn failure(text: &str) {
    line_err("");
    line_err(&styled(&format!("✗ {text}"), Style::RedBold));
}

pub fn warning(text: &str) {
    indented(true, 2, &styled(&format!("! {text}"), Style::Yellow));
}

/// Something that needs attention, beneath a verdict.
pub fn attention(text: &str) {
    indented(false, 2, &styled(&format!("! {text}"), Style::Yellow));
}

pub fn verdict(text: &str, system: &str, matches: bool) {
    let (mark, style) = if matches {
        ('✓', Style::GreenBold)
    } else {
        ('!', Style::YellowBold)
    };
    line(&format!(
        "{} {}",
        styled(&format!("{mark} {text}"), style),
        dim(&format!("({system})"))
    ));
}

fn columns() -> usize {
    // SAFETY: winsize is plain numbers, and TIOCGWINSZ fills the one it is given.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0;
    if ok && size.ws_col > 0 {
        size.ws_col as usize
    } else {
        80
    }
}

/// Redraws the two reserved rows, erasing each in full.
fn redraw(snapshot: &[String], repaint: bool) {
    let mut frame = String::new();
    if repaint {
        frame.push_str("\u{1b}[2A");
    }
    // One column stays free so the terminal never wraps a row.
    let width = columns().saturating_sub(1).max(1);
    for row in 0..2 {
        let text = snapshot.get(row).map(String::as_str).unwrap_or("");
        frame.push_str("\r\u{1b}[2K");
        frame.push_str(&styled(&fit(&format!("    {text}"), width), Style::Dimmed));
        frame.push('\n');
    }
    emit(false, &frame);
}

struct Monitor {
    progress: progress::State,
    shown: Vec<String>,
    drawn: bool,
    at: Instant,
    live: bool,
    quiet: bool,
}

impl Monitor {
    fn new(inputs: &[String]) -> Self {
        Monitor {
            progress: progress::initial(inputs),
            shown: Vec::new(),
            drawn: false,
            at: Instant::now() - Duration::from_secs(1),
            live: live(),
            quiet: capturing(),
        }
    }

    fn show(&mut self, snapshot: Vec<String>) {
        if self.quiet {
            // Nothing is printed, but the snapshot still counts as shown.
        } else if self.live {
            redraw(&snapshot, self.drawn);
        } else {
            for text in snapshot.iter().filter(|text| !self.shown.contains(text)) {
                detail(text);
            }
        }
        self.shown = snapshot;
        self.drawn = true;
        self.at = Instant::now();
    }

    /// Counts and logs are throttled; input and derivation changes are immediate.
    fn advance(&mut self, line: &str) {
        let state = progress::advance(&self.progress, progress::event(line).as_ref());
        let snapshot = progress::snapshot(&state, self.live);
        let keys = |logs: &[(String, String)]| -> Vec<String> {
            logs.iter().map(|(key, _)| key.clone()).collect()
        };
        let changed = state.input != self.progress.input
            || state.active != self.progress.active
            || state.current != self.progress.current
            || keys(&state.logs) != keys(&self.progress.logs);
        let ready = changed || self.at.elapsed() >= Duration::from_secs(1);
        self.progress = state;
        if snapshot != self.shown && ready && (!snapshot.is_empty() || (self.live && self.drawn)) {
            self.show(snapshot);
        }
    }

    fn finish(&mut self) {
        if self.quiet {
            return;
        }
        if self.live {
            if self.drawn {
                emit(false, CLEARED);
            }
            return;
        }
        let snapshot = progress::snapshot(&self.progress, false);
        if snapshot != self.shown {
            self.show(snapshot);
        }
    }
}

/// Runs a tool with curated output. Its raw output stays hidden unless it
/// fails; then the last diagnostic lines are part of the error.
pub fn stream(command: &mut Command, progress: bool, inputs: &[String]) -> Result<(), String> {
    let (mut child, reader) = process::spawn_merged(command)?;

    let mut monitor = progress.then(|| Monitor::new(inputs));
    let mut output = String::new();
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buffer);
        output.push_str(&text);
        for part in text.trim_end_matches('\n').split('\r') {
            match monitor.as_mut() {
                Some(monitor) => monitor.advance(part),
                None => detail(&strip_ansi(part)),
            }
        }
    }
    if let Some(monitor) = monitor.as_mut() {
        monitor.finish();
    }
    let status = child
        .wait()
        .map_err(|err| format!("cannot wait for {}: {err}", process::name(command)))?;
    if status.success() {
        return Ok(());
    }
    let decoded = strip_ansi(&output)
        .replace('\r', "\n")
        .lines()
        .map(progress::diagnostic)
        .collect::<Vec<_>>()
        .join("\n")
        .replace('\r', "\n");
    Err(process::failure(&process::name(command), status, &decoded))
}
