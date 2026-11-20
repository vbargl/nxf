//! Child processes: running them, with or without a deadline, the one shape
//! of their failures, and the atomic write of a file.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use os_pipe::PipeReader;

use crate::text::{last_line, plain};

/// How many of the last lines a program said go into the error of its failure.
const TAIL: usize = 8;
/// What a script that the deadline cut short says for itself.
pub const TIMED_OUT: &str = "timed out";
/// How long the output of a program that ended is waited for.
const DRAIN: Duration = Duration::from_secs(1);

/// The program of a command, without the directory it is run from.
pub fn name(command: &Command) -> String {
    let program = Path::new(command.get_program());
    program
        .file_name()
        .unwrap_or(program.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The one spelling of a program that could not be started.
pub fn cannot_run(command: &Command, err: impl std::fmt::Display) -> String {
    format!(
        "cannot run {}: {err}",
        command.get_program().to_string_lossy()
    )
}

/// The last `TAIL` lines of a text that are not empty.
pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(TAIL)..].join("\n")
}

/// How a program failed, in the one shape of nxf: its name and how it ended,
/// then the last lines it said.
pub fn failure(program: &str, status: ExitStatus, said: &str) -> String {
    let ended = match status.code() {
        Some(code) => format!("{program} exited with status {code}."),
        None => format!("{program} was stopped by a signal."),
    };
    match tail(said) {
        said if said.is_empty() => ended,
        said => format!("{ended}\n{said}"),
    }
}

/// Whether a line is the first line of a `failure`.
pub fn is_failure_line(line: &str) -> bool {
    line.contains(" exited with status ") || line.ends_with(" was stopped by a signal.")
}

/// Runs a command to its end with no input, whatever its status.
pub fn capture(command: &mut Command) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .output()
        .map_err(|err| cannot_run(command, err))
}

/// Runs a command to its end with no input; a status other than success is
/// its failure.
pub fn run(command: &mut Command) -> Result<Output, String> {
    let output = capture(command)?;
    if output.status.success() {
        return Ok(output);
    }
    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Err(failure(&name(command), output.status, &plain(&said)))
}

/// How a command ended within its time.
pub struct Finished {
    /// None when the deadline cut it short.
    pub status: Option<ExitStatus>,
    pub stdout: String,
    /// Empty when the output was merged.
    pub stderr: String,
}

/// What a pipe holds so far, read on a thread of its own so that a full pipe
/// cannot stall the program.
struct Drain {
    held: Arc<Mutex<Vec<u8>>>,
    ended: mpsc::Receiver<()>,
}

impl Drain {
    fn start(mut pipe: impl Read + Send + 'static) -> Self {
        let held = Arc::new(Mutex::new(Vec::new()));
        let (ended_send, ended) = mpsc::channel();
        let keep = Arc::clone(&held);
        std::thread::spawn(move || {
            let mut chunk = [0; 4096];
            while let Ok(count @ 1..) = pipe.read(&mut chunk) {
                keep.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .extend_from_slice(&chunk[..count]);
            }
            let _ = ended_send.send(());
        });
        Drain { held, ended }
    }

    /// The text read so far, after waiting for the end of the pipe when asked
    /// to: a child of the program may hold the pipe open after it ended.
    fn text(self, wait: bool) -> String {
        if wait {
            let _ = self.ended.recv_timeout(DRAIN);
        }
        let held = self
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        String::from_utf8_lossy(&held).into_owned()
    }
}

/// The readers of the output of a started command.
struct Pipes {
    stdout: PipeReader,
    /// None when the standard error goes where the standard output goes.
    stderr: Option<PipeReader>,
}

/// Starts a command with no input and its output in pipes.
fn spawn_piped(command: &mut Command, merged: bool) -> Result<(Child, Pipes), String> {
    let pipes = || os_pipe::pipe().map_err(|err| format!("cannot create a pipe: {err}"));
    let (out_reader, out_writer) = pipes()?;
    let (err_reader, err_writer) = if merged {
        (None, out_writer.try_clone().map_err(|err| err.to_string())?)
    } else {
        let (reader, writer) = pipes()?;
        (Some(reader), writer)
    };
    let child = command
        .stdin(Stdio::null())
        .stdout(out_writer)
        .stderr(err_writer)
        .spawn()
        .map_err(|err| cannot_run(command, err))?;
    // The command keeps the write ends until it is dropped or reconfigured.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    Ok((
        child,
        Pipes {
            stdout: out_reader,
            stderr: err_reader,
        },
    ))
}

/// Starts a command with no input, and its standard error merged into its
/// standard output, which the caller reads as it comes.
pub fn spawn_merged(command: &mut Command) -> Result<(Child, PipeReader), String> {
    let (child, pipes) = spawn_piped(command, true)?;
    Ok((child, pipes.stdout))
}

/// Runs a command to its end with the given input, whatever its status.
pub fn feed(command: &mut Command, input: &str) -> Result<Output, String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| cannot_run(command, err))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    child
        .wait_with_output()
        .map_err(|err| cannot_run(command, err))
}

/// Runs a command with no input and kills it at the deadline. With `merged`
/// its standard error goes where its standard output goes.
pub fn run_within(
    command: &mut Command,
    deadline: Instant,
    merged: bool,
) -> Result<Finished, String> {
    let (mut child, pipes) = spawn_piped(command, merged)?;
    let out_reader = pipes.stdout;
    let err_reader = pipes.stderr;
    let stdout = Drain::start(out_reader);
    let stderr = err_reader.map(Drain::start);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // A killed program may have children that hold its pipes open.
    let ended = status.is_some();
    Ok(Finished {
        status,
        stdout: stdout.text(ended),
        stderr: stderr.map_or_else(String::new, |stderr| stderr.text(ended)),
    })
}

/// Runs a script of the configuration until the deadline, with the variables
/// added to its environment. The error says why it did not pass, and is none
/// when the deadline cut it short: what a script says itself is never taken
/// for that.
pub fn script(
    path: &Path,
    deadline: Instant,
    variables: &[(&str, &Path)],
) -> Result<(), Option<String>> {
    let mut command = Command::new(path);
    command.envs(variables.iter().copied());
    let done = run_within(&mut command, deadline, true).map_err(Some)?;
    let Some(status) = done.status else {
        return Err(None);
    };
    if status.success() {
        return Ok(());
    }
    Err(Some(
        match (last_line(&plain(&done.stdout)), status.code()) {
            (Some(line), _) => line,
            (None, Some(code)) => format!("exit status {code}"),
            (None, None) => "killed by a signal".to_string(),
        },
    ))
}

/// Writes a file whole or not at all: to a temporary file beside it, which
/// replaces it. The directory is made when it is not there.
pub fn write_atomic(file: &Path, text: &str) -> std::io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&temporary, text))
        .and_then(|()| std::fs::rename(&temporary, file))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
}
