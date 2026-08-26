use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GtpCommand {
    pub identifier: u64,
    pub name: String,
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GtpResponse {
    pub identifier: Option<u64>,
    pub success: bool,
    pub content: String,
}

#[derive(Debug, Error)]
pub enum GtpError {
    #[error("GTP command name must not be empty")]
    EmptyCommandName,
    #[error("GTP response did not begin with '=' or '?'")]
    InvalidResponse,
    #[error("GTP response identifier is invalid")]
    InvalidIdentifier,
    #[error("GTP engine process could not be started: {0}")]
    ProcessStart(#[from] std::io::Error),
    #[error("GTP engine does not expose standard input")]
    MissingStandardInput,
    #[error("GTP engine does not expose standard output")]
    MissingStandardOutput,
    #[error("GTP engine stopped before completing a response")]
    UnexpectedEndOfStream,
    #[error("GTP command `{command}` timed out after {timeout:?}")]
    CommandTimeout { command: String, timeout: Duration },
    #[error("GTP transport cannot be reused after a timed-out command")]
    TransportPoisoned,
    #[error("this transport does not support streaming commands")]
    UnsupportedStreaming,
}

impl GtpCommand {
    pub fn format(&self) -> Result<String, GtpError> {
        if self.name.trim().is_empty() {
            return Err(GtpError::EmptyCommandName);
        }
        let arguments = self.arguments.join(" ");
        let separator = if arguments.is_empty() { "" } else { " " };
        Ok(format!(
            "{} {}{}{}\n",
            self.identifier, self.name, separator, arguments
        ))
    }
}

pub fn parse_response(lines: impl IntoIterator<Item = String>) -> Result<GtpResponse, GtpError> {
    let mut lines = lines.into_iter();
    let first_line = lines.next().ok_or(GtpError::UnexpectedEndOfStream)?;
    let mut first_line_characters = first_line.chars();
    let success = match first_line_characters.next() {
        Some('=') => true,
        Some('?') => false,
        _ => return Err(GtpError::InvalidResponse),
    };
    let response_header = first_line_characters.as_str().trim_start();
    let mut header_tokens = response_header.splitn(2, char::is_whitespace);
    let first_token = header_tokens.next().unwrap_or_default();
    let (identifier, initial_content) = if first_token.is_empty() {
        (None, String::new())
    } else if let Ok(identifier) = first_token.parse() {
        (
            Some(identifier),
            header_tokens.next().unwrap_or_default().to_owned(),
        )
    } else {
        (None, response_header.to_owned())
    };

    let mut response_lines = Vec::new();
    if !initial_content.is_empty() {
        response_lines.push(initial_content);
    }
    response_lines.extend(lines.take_while(|line| !line.is_empty()));
    Ok(GtpResponse {
        identifier,
        success,
        content: response_lines.join("\n"),
    })
}

pub struct GtpProcessSupervisor {
    child: Child,
    standard_input: ChildStdin,
    receiver: std::sync::mpsc::Receiver<String>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    stream_closed: Arc<AtomicBool>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_identifier: u64,
    /// A timeout means stdout may still contain a late response for the timed
    /// out command. Refusing reuse prevents that response being misattributed
    /// to a later command on the shared stream.
    poisoned: bool,
}

/// Number of trailing stderr lines retained for diagnostics.
const STDERR_TAIL_LINES: usize = 64;
/// Safety deadline for a normal request/response GTP command. UI callers run
/// commands asynchronously, but a malfunctioning engine must never leave a
/// worker blocked forever either.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// Search commands may legitimately run longer than setup/console operations on
/// a cold model or constrained device. They remain bounded, but need their own
/// policy rather than inheriting the short protocol deadline.
pub const SEARCH_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// Process cleanup must never indefinitely retain a GPUI worker.
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(2);

impl GtpProcessSupervisor {
    /// Starts the engine inheriting the caller's working directory.
    pub fn start(executable: &str, arguments: &[String]) -> Result<Self, GtpError> {
        Self::start_in(executable, arguments, None)
    }

    /// Starts the engine in an explicit working directory. KataGo's generated
    /// config writes relative `logDir = katago_logs`; from a non-writable cwd
    /// (e.g. a packaged `.app` bundle) the engine aborts during startup, which
    /// surfaces as a failed handshake. Callers that know a writable directory
    /// (the app config dir) must pass it here.
    pub fn start_in(
        executable: &str,
        arguments: &[String],
        current_dir: Option<&std::path::Path>,
    ) -> Result<Self, GtpError> {
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = current_dir {
            command.current_dir(dir);
        }
        let mut child = command.spawn()?;
        let standard_input = child.stdin.take().ok_or(GtpError::MissingStandardInput)?;
        let standard_output = child.stdout.take().ok_or(GtpError::MissingStandardOutput)?;
        let standard_error = child.stderr.take();
        let (sender, receiver) = std::sync::mpsc::channel();
        let stream_closed = Arc::new(AtomicBool::new(false));
        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

        let closed_flag = stream_closed.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(standard_output);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let normalized = line.trim_end_matches(['\r', '\n']).to_owned();
                        if sender.send(normalized).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            closed_flag.store(true, Ordering::SeqCst);
        });

        // KataGo and other engines log heavily to stderr. A piped-but-unread
        // stderr fills the OS pipe buffer and blocks the engine, which then
        // stops answering GTP commands. Drain it continuously and keep a small
        // tail for diagnostics.
        let err_tail = stderr_tail.clone();
        let stderr_thread = standard_error.map(|stderr| {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let normalized = line.trim_end_matches(['\r', '\n']).to_owned();
                            let mut tail = err_tail.lock().unwrap_or_else(|p| p.into_inner());
                            if tail.len() >= STDERR_TAIL_LINES {
                                tail.pop_front();
                            }
                            tail.push_back(normalized);
                        }
                    }
                }
            })
        });

        Ok(Self {
            child,
            standard_input,
            receiver,
            reader_thread: Some(reader_thread),
            stderr_thread,
            stream_closed,
            stderr_tail,
            next_identifier: 1,
            poisoned: false,
        })
    }

    /// Sends a bounded GTP command with the default safety deadline.
    pub fn send(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<String>,
    ) -> Result<GtpResponse, GtpError> {
        self.send_with_timeout(name, arguments, DEFAULT_COMMAND_TIMEOUT)
    }

    /// Sends a bounded GTP command and waits only until `timeout` for its
    /// complete, identifier-matched response. This is deliberately bounded:
    /// an engine which writes a header but never terminates its response used
    /// to freeze the GPUI thread forever.
    pub fn send_with_timeout(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<String>,
        timeout: Duration,
    ) -> Result<GtpResponse, GtpError> {
        if self.poisoned {
            return Err(GtpError::TransportPoisoned);
        }
        let command = GtpCommand {
            identifier: self.next_identifier,
            name: name.into(),
            arguments,
        };
        self.next_identifier += 1;
        if self.is_stream_closed() {
            return Err(GtpError::UnexpectedEndOfStream);
        }
        self.standard_input
            .write_all(command.format()?.as_bytes())?;
        self.standard_input.flush()?;

        let deadline = Instant::now() + timeout;
        let expected_identifier = command.identifier;
        let mut response_lines = Vec::new();
        let mut header_seen = false;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.poisoned = true;
                return Err(GtpError::CommandTimeout {
                    command: command.name,
                    timeout,
                });
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(normalized_line) => {
                    if !header_seen {
                        // A bounded command must never consume a stream record
                        // as its own response. We can recognize numbered GTP
                        // headers precisely; identifier-less engines are only
                        // safe when no stream is running and are accepted as
                        // the current response for compatibility.
                        if !normalized_line.starts_with('=') && !normalized_line.starts_with('?') {
                            continue;
                        }
                        let after_marker = &normalized_line[1..];
                        let header_token =
                            after_marker.split_whitespace().next().unwrap_or_default();
                        if let Ok(parsed_identifier) = header_token.parse::<u64>()
                            && parsed_identifier != expected_identifier
                        {
                            continue;
                        }
                        header_seen = true;
                        response_lines.push(normalized_line);
                        continue;
                    }
                    let is_complete = normalized_line.is_empty();
                    response_lines.push(normalized_line);
                    if is_complete {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    self.poisoned = true;
                    return Err(GtpError::CommandTimeout {
                        command: command.name,
                        timeout,
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(GtpError::UnexpectedEndOfStream);
                }
            }
        }
        parse_response(response_lines)
    }

    /// Writes a command line without waiting for the response, for streaming
    /// commands (e.g. `kata-analyze`) whose output continues until `stop`.
    pub fn send_streaming(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<String>,
    ) -> Result<(), GtpError> {
        if self.poisoned {
            return Err(GtpError::TransportPoisoned);
        }
        let command = GtpCommand {
            identifier: self.next_identifier,
            name: name.into(),
            arguments,
        };
        self.next_identifier += 1;
        self.standard_input
            .write_all(command.format()?.as_bytes())?;
        self.standard_input.flush()?;
        Ok(())
    }

    /// Waits up to `timeout` for the next output line from a streaming
    /// command.
    pub fn recv_line_timeout(&mut self, timeout: std::time::Duration) -> Option<String> {
        self.receiver.recv_timeout(timeout).ok()
    }

    /// True once the engine's stdout reached EOF or its pipe was closed —
    /// the engine has exited or stopped producing output.
    pub fn is_stream_closed(&self) -> bool {
        self.stream_closed.load(Ordering::SeqCst)
    }

    /// Blocks until the engine's output stream closes or `timeout` expires.
    pub fn wait_closed(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_stream_closed() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.is_stream_closed()
    }

    /// The trailing engine stderr lines, for diagnosing abrupt exits.
    pub fn stderr_tail(&self) -> String {
        let tail = self.stderr_tail.lock().unwrap_or_else(|p| p.into_inner());
        tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    pub fn stop_with_timeout(&mut self, timeout: Duration) -> Result<(), std::io::Error> {
        // Killing the direct child closes its own descriptors, but descendants
        // may inherit stdout/stderr. Never unconditionally join reader threads:
        // doing so can block a GPUI worker until every descendant exits.
        let kill_result = self.child.kill();
        self.stream_closed.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait()? {
                Some(_) => break,
                None if Instant::now() >= deadline => break,
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        if self
            .reader_thread
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
            && let Some(handle) = self.reader_thread.take()
        {
            let _ = handle.join();
        }
        if self
            .stderr_thread
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
            && let Some(handle) = self.stderr_thread.take()
        {
            let _ = handle.join();
        }
        // Dropping unfinished JoinHandles detaches them. They own only the pipe
        // readers and shared diagnostics, not the supervisor or UI state.
        self.reader_thread.take();
        self.stderr_thread.take();
        match kill_result {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn stop(&mut self) -> Result<(), std::io::Error> {
        self.stop_with_timeout(DEFAULT_STOP_TIMEOUT)
    }
}

impl Drop for GtpProcessSupervisor {
    fn drop(&mut self) {
        // `Child` does not kill its subprocess on drop. A failed handshake
        // therefore leaked a live KataGo process before this safeguard.
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbered_commands_for_a_process_transport() {
        let command = GtpCommand {
            identifier: 7,
            name: "play".to_owned(),
            arguments: vec!["B".to_owned(), "D4".to_owned()],
        };
        assert_eq!(command.format().unwrap(), "7 play B D4\n");
    }

    #[test]
    fn parses_multiline_success_and_error_responses() {
        assert_eq!(
            parse_response(["=3 KataGo".to_owned(), "1.16.4".to_owned(), "".to_owned()]).unwrap(),
            GtpResponse {
                identifier: Some(3),
                success: true,
                content: "KataGo\n1.16.4".to_owned(),
            }
        );
        assert_eq!(
            parse_response(["?4 illegal move".to_owned(), "".to_owned()]).unwrap(),
            GtpResponse {
                identifier: Some(4),
                success: false,
                content: "illegal move".to_owned(),
            }
        );
    }
}

/// A streaming analysis process: one command starts a continuous line stream
/// (KataGo `kata-analyze`, Leela `lz-analyze`). A background thread reads the
/// child's stdout and delivers every line over a channel; the caller polls
/// lines and decides when the stream is done (blank line, `isDuringSearch:
/// false`, or an explicit `stop`). Dropping the stream kills the child.
pub struct AnalysisStream {
    child: std::process::Child,
    standard_input: ChildStdin,
    receiver: std::sync::mpsc::Receiver<String>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    stream_closed: Arc<AtomicBool>,
}

impl AnalysisStream {
    /// Starts an analysis process inheriting the caller's working directory.
    pub fn start(executable: &str, arguments: &[String]) -> Result<Self, GtpError> {
        Self::start_in(executable, arguments, None)
    }

    /// Starts an analysis process in an explicit working directory, so a
    /// packaged app (non-writable cwd) can still run KataGo whose config
    /// writes a relative `logDir`.
    pub fn start_in(
        executable: &str,
        arguments: &[String],
        current_dir: Option<&std::path::Path>,
    ) -> Result<Self, GtpError> {
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(dir) = current_dir {
            command.current_dir(dir);
        }
        let mut child = command.spawn()?;
        let standard_input = child.stdin.take().ok_or(GtpError::MissingStandardInput)?;
        let standard_output = child.stdout.take().ok_or(GtpError::MissingStandardOutput)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let stream_closed = Arc::new(AtomicBool::new(false));
        let closed_flag = stream_closed.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(standard_output);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let normalized = line.trim_end_matches(['\r', '\n']).to_owned();
                        if sender.send(normalized).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            closed_flag.store(true, Ordering::SeqCst);
        });
        Ok(Self {
            child,
            standard_input,
            receiver,
            reader_thread: Some(reader_thread),
            stream_closed,
        })
    }

    /// Sends one command line to the process (no identifier framing; the
    /// analysis protocol is a plain continuous stream per command).
    pub fn send_command(&mut self, command: &str) -> std::io::Result<()> {
        self.standard_input.write_all(command.as_bytes())?;
        self.standard_input.write_all(b"\n")?;
        self.standard_input.flush()?;
        Ok(())
    }

    /// Sends a bounded setup request and consumes its complete GTP response
    /// before the caller enters continuous analysis mode. Fresh analysis uses
    /// this for replay and `kata-set-param`, so setup headers cannot leak into
    /// the later `info move` stream.
    pub fn send_request(
        &mut self,
        command: &str,
        timeout: Duration,
    ) -> Result<GtpResponse, GtpError> {
        self.send_command(command)?;
        let deadline = Instant::now() + timeout;
        let mut response_lines = Vec::new();
        let mut header_seen = false;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(GtpError::CommandTimeout {
                    command: command.to_owned(),
                    timeout,
                });
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(line) if !header_seen => {
                    if line.starts_with('=') || line.starts_with('?') {
                        header_seen = true;
                        response_lines.push(line);
                    }
                }
                Ok(line) => {
                    let complete = line.is_empty();
                    response_lines.push(line);
                    if complete {
                        return parse_response(response_lines);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(GtpError::CommandTimeout {
                        command: command.to_owned(),
                        timeout,
                    });
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(GtpError::UnexpectedEndOfStream);
                }
            }
        }
    }

    /// Returns the next line without blocking, or `None` when nothing is
    /// buffered yet.
    pub fn next_line(&mut self) -> Option<String> {
        self.receiver.try_recv().ok()
    }

    /// Waits up to `timeout` for the next line.
    pub fn recv_line_timeout(&mut self, timeout: std::time::Duration) -> Option<String> {
        self.receiver.recv_timeout(timeout).ok()
    }

    /// True once the analysis process's stdout closed (engine exited).
    pub fn is_stream_closed(&self) -> bool {
        self.stream_closed.load(Ordering::SeqCst)
    }

    /// Blocks until the analysis stream closes or `timeout` expires.
    pub fn wait_closed(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_stream_closed() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.is_stream_closed()
    }

    /// Asks a searching engine to stop and emit its final analysis.
    pub fn stop(&mut self) -> std::io::Result<()> {
        self.send_command("stop")
    }

    /// Kills the process and joins the reader thread.
    pub fn kill(&mut self) -> std::io::Result<()> {
        let kill_result = self.child.kill();
        self.stream_closed.store(true, Ordering::SeqCst);
        self.join_reader();
        kill_result
    }

    fn join_reader(&mut self) {
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AnalysisStream {
    fn drop(&mut self) {
        self.stream_closed.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        self.join_reader();
    }
}

#[cfg(test)]
mod stream_tests {
    use super::{AnalysisStream, GtpError, GtpProcessSupervisor};
    use std::time::{Duration, Instant};

    /// Resolves a Python interpreter for the subprocess fixture. Windows
    /// runners can expose an App Execution Alias named `python` that launches
    /// the Microsoft Store instead of an interpreter, so accept only candidates
    /// whose version output identifies CPython.
    fn python() -> Option<&'static str> {
        ["python3", "python"].into_iter().find(|candidate| {
            let Ok(output) = std::process::Command::new(candidate)
                .arg("--version")
                .output()
            else {
                return false;
            };
            if !output.status.success() {
                return false;
            }
            let version = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            version.contains("Python ")
        })
    }

    #[test]
    fn streams_lines_from_a_subprocess_until_finished() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping stream test");
            return;
        };
        let script = "\
import sys, time
sys.stdout.write('= ready\\n'); sys.stdout.flush()
time.sleep(0.02)
sys.stdout.write('{\"id\":1,\"move\":\"D4\",\"isDuringSearch\":true}\\n'); sys.stdout.flush()
time.sleep(0.02)
sys.stdout.write('{\"id\":1,\"move\":\"D4\",\"isDuringSearch\":false}\\n'); sys.stdout.flush()
";
        let mut stream = AnalysisStream::start(python, &["-c".to_owned(), script.to_owned()])
            .expect("stream process starts");

        stream
            .send_command("kata-analyze")
            .expect("command is sent");

        let mut lines = Vec::new();
        // Windows runners start python3 slowly (interpreter + antivirus scan),
        // so poll with a generous budget instead of one short timeout.
        for _ in 0..20 {
            match stream.recv_line_timeout(Duration::from_millis(500)) {
                Some(line) => {
                    lines.push(line);
                    if lines
                        .iter()
                        .any(|line| line.contains("\"isDuringSearch\":false"))
                    {
                        break;
                    }
                }
                None => continue,
            }
        }

        assert!(
            lines.len() >= 3,
            "expected at least 3 streamed lines, got {lines:?}"
        );
        assert!(lines[0].contains("ready"));
        stream.kill().expect("stream is killed");
    }

    #[test]
    fn analysis_stream_reads_setup_response_before_stream_records() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping setup framing test");
            return;
        };
        let script = "\
import sys
for line in sys.stdin:
    command = line.strip()
    if command == 'boardsize 9':
        print('= setup-ok'); print(); sys.stdout.flush()
    elif command == 'kata-analyze':
        print('info move D4 visits 10 winrate 0.5 pv D4'); sys.stdout.flush()
";
        let mut stream = AnalysisStream::start(python, &["-c".to_owned(), script.to_owned()])
            .expect("stream process starts");
        let response = stream
            .send_request("boardsize 9", Duration::from_secs(1))
            .expect("setup response is framed");
        assert!(response.success);
        assert_eq!(response.content, "setup-ok");
        stream.send_command("kata-analyze").expect("stream starts");
        assert!(
            stream
                .recv_line_timeout(Duration::from_secs(1))
                .is_some_and(|line| line.starts_with("info move")),
            "the setup response must not leak into the analysis stream"
        );
        stream.kill().ok();
    }

    #[test]
    fn stop_asks_the_engine_to_finish() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping stop test");
            return;
        };
        let script = "\
import sys
for line in sys.stdin:
    sys.stdout.write('{\"id\":1,\"move\":\"D4\",\"isDuringSearch\":false}\\n')
    sys.stdout.flush()
";
        let mut stream = AnalysisStream::start(python, &["-c".to_owned(), script.to_owned()])
            .expect("stream process starts");

        stream
            .send_command("kata-analyze")
            .expect("command is sent");
        stream.stop().expect("stop is sent");

        let line = (0..10)
            .find_map(|_| stream.recv_line_timeout(Duration::from_millis(500)))
            .expect("the engine answers the stop");
        assert!(line.contains("isDuringSearch"));
        stream.kill().expect("stream is killed");
    }

    /// Regression: verbose engines (KataGo logs heavily to stderr) must not
    /// deadlock once the stderr pipe buffer fills. Before the fix the engine
    /// blocked writing stderr and never answered GTP commands, surfacing as
    /// "engine stopped before completing a response".
    #[test]
    fn drains_engine_stderr_so_verbose_engines_do_not_deadlock() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping stderr drain test");
            return;
        };
        let script = "\
import sys
sys.stderr.write('k' * 400_000)
sys.stderr.flush()
print('=1 VerboseEngine'); print(); sys.stdout.flush()
";
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut supervisor =
                GtpProcessSupervisor::start(python, &["-c".to_owned(), script.to_owned()])
                    .expect("supervisor starts");
            let response = supervisor.send("name", Vec::new());
            sender.send(response).ok();
            supervisor.stop().ok();
        });
        let response = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("engine must answer before its stderr pipe fills");
        assert!(
            matches!(response, Ok(ref r) if r.success && r.content.contains("VerboseEngine")),
            "bounded handshake must complete: {response:?}"
        );
        worker.join().ok();
    }

    /// Regression: a bounded command issued while `kata-analyze` is still
    /// streaming must not swallow `info move` records. Before the fix the
    /// shared channel made `send` consume stream lines into its own response,
    /// corrupting the analysis and (when no terminator arrived) blocking until
    /// the engine was killed.
    #[test]
    fn bounded_commands_skip_inflight_stream_lines() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping bounded-command test");
            return;
        };
        let script = "\
import sys, threading, time
emitting = threading.Event()
def streamer():
    emitting.wait()
    i = 0
    while True:
        i += 1
        print('info move D4 visits %d winrate 0.5 scoreLead 1.0 pv D4 Q16' % i)
        sys.stdout.flush()
        time.sleep(0.05)
threading.Thread(target=streamer, daemon=True).start()
for line in sys.stdin:
    parts = line.strip().split()
    ident = parts[0]
    name = parts[1] if len(parts) > 1 else ''
    if name == 'name':
        print('=%s FakeEngine' % ident); print(); sys.stdout.flush()
    elif name == 'version':
        print('=%s 1.0' % ident); print(); sys.stdout.flush()
    elif name in ('boardsize', 'clear_board', 'kata-set-param'):
        print('=%s' % ident); print(); sys.stdout.flush()
    elif name == 'kata-analyze':
        print('=%s' % ident); sys.stdout.flush()
        emitting.set()
";
        let mut supervisor =
            GtpProcessSupervisor::start(python, &["-c".to_owned(), script.to_owned()])
                .expect("supervisor starts");
        for (name, args) in [
            ("name", vec![]),
            ("version", vec![]),
            ("boardsize", vec!["19".to_owned()]),
            ("clear_board", vec![]),
        ] {
            let response = supervisor
                .send(name, args)
                .expect("handshake command completes");
            assert!(response.success, "handshake must succeed: {response:?}");
        }
        supervisor
            .send_streaming("kata-analyze", Vec::new())
            .expect("stream starts");
        // Let several `info move` records queue up on the shared channel.
        std::thread::sleep(Duration::from_millis(180));

        let response = supervisor
            .send(
                "kata-set-param",
                vec!["maxVisits".to_owned(), "500".to_owned()],
            )
            .expect("mid-stream bounded command completes");
        assert!(
            !response.content.contains("info move"),
            "stream records must not leak into a bounded response: {:?}",
            response.content
        );

        // The streaming tail remains readable through the streaming reader.
        let tail: Vec<String> = (0..20)
            .filter_map(|_| supervisor.recv_line_timeout(Duration::from_millis(100)))
            .filter(|line| line.contains("info move"))
            .collect();
        assert!(
            !tail.is_empty(),
            "streaming reader must still observe later info records"
        );
        supervisor.stop().ok();
    }

    /// Regression for the UI-freeze path: an engine can write a response
    /// header and then never emit the blank terminator. A bounded command must
    /// return a typed timeout rather than block its caller forever.
    #[test]
    fn incomplete_response_times_out_instead_of_hanging() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping timeout test");
            return;
        };
        let script = "\
import sys, time
for line in sys.stdin:
    ident = line.split()[0]
    print('=%s PartialResponse' % ident)
    sys.stdout.flush()
    time.sleep(60)
";
        let mut supervisor =
            GtpProcessSupervisor::start(python, &["-c".to_owned(), script.to_owned()])
                .expect("supervisor starts");
        let timeout = Duration::from_millis(150);
        let error = supervisor
            .send_with_timeout("name", Vec::new(), timeout)
            .expect_err("incomplete response must not block forever");
        assert!(matches!(
            error,
            GtpError::CommandTimeout { command, timeout: observed }
                if command == "name" && observed == timeout
        ));
        supervisor.stop().ok();
    }

    /// Regression: when the engine exits, the transport must expose the closed
    /// state promptly (instead of an indefinite `None` timeout stream) and
    /// bounded commands must fail with the documented error.
    #[test]
    fn reports_stream_closure_for_exited_engines() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping closure test");
            return;
        };
        let script = "\
import sys
print('=1 ShortLived'); print(); sys.stdout.flush()
sys.exit(0)
";
        let mut supervisor =
            GtpProcessSupervisor::start(python, &["-c".to_owned(), script.to_owned()])
                .expect("supervisor starts");
        let response = supervisor
            .send("name", Vec::new())
            .expect("first handshake completes");
        assert!(response.success);
        assert!(
            supervisor.wait_closed(Duration::from_secs(5)),
            "the engine exited, so the stream must close"
        );
        assert!(matches!(
            supervisor.send("version", Vec::new()),
            Err(GtpError::UnexpectedEndOfStream)
        ));
        supervisor.stop().ok();
    }

    #[test]
    fn timed_out_command_poisoned_transport_rejects_later_commands() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping timeout poison test");
            return;
        };
        let script = "\
import sys, time
for line in sys.stdin:
    ident = line.split()[0]
    print('=%s PartialResponse' % ident)
    sys.stdout.flush()
    time.sleep(60)
";
        let mut supervisor =
            GtpProcessSupervisor::start(python, &["-c".to_owned(), script.to_owned()])
                .expect("supervisor starts");
        let timeout = Duration::from_millis(150);
        assert!(matches!(
            supervisor.send_with_timeout("name", Vec::new(), timeout),
            Err(GtpError::CommandTimeout { .. })
        ));
        assert!(matches!(
            supervisor.send("version", Vec::new()),
            Err(GtpError::TransportPoisoned)
        ));
        assert!(matches!(
            supervisor.send_streaming("kata-analyze", Vec::new()),
            Err(GtpError::TransportPoisoned)
        ));
        supervisor.stop().ok();
    }

    #[test]
    fn stop_is_bounded_when_a_descendant_inherits_output_pipes() {
        if cfg!(windows) {
            eprintln!("Unix Python pipe fixture is not run on Windows");
            return;
        }
        let Some(python) = python() else {
            eprintln!("Python interpreter not found; skipping bounded stop test");
            return;
        };
        let descendant = "import time; time.sleep(2)";
        let script = format!(
            "import subprocess, sys\nsubprocess.Popen([{python:?}, '-c', {descendant:?}])\nfor _ in sys.stdin: pass\n"
        );
        let mut supervisor = GtpProcessSupervisor::start(python, &["-c".to_owned(), script])
            .expect("supervisor starts");
        let started = Instant::now();
        supervisor
            .stop_with_timeout(Duration::from_millis(150))
            .expect("direct child stops within deadline");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "reader joins must not wait for inherited descendant pipes"
        );
    }
}
