use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
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
    next_identifier: u64,
}

impl GtpProcessSupervisor {
    pub fn start(executable: &str, arguments: &[String]) -> Result<Self, GtpError> {
        let mut child = Command::new(executable)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let standard_input = child.stdin.take().ok_or(GtpError::MissingStandardInput)?;
        let standard_output = child.stdout.take().ok_or(GtpError::MissingStandardOutput)?;
        let (sender, receiver) = std::sync::mpsc::channel();
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
        });
        Ok(Self {
            child,
            standard_input,
            receiver,
            reader_thread: Some(reader_thread),
            next_identifier: 1,
        })
    }

    pub fn send(
        &mut self,
        name: impl Into<String>,
        arguments: Vec<String>,
    ) -> Result<GtpResponse, GtpError> {
        let command = GtpCommand {
            identifier: self.next_identifier,
            name: name.into(),
            arguments,
        };
        self.next_identifier += 1;
        self.standard_input
            .write_all(command.format()?.as_bytes())?;
        self.standard_input.flush()?;

        let mut response_lines = Vec::new();
        loop {
            match self.receiver.recv() {
                Ok(normalized_line) => {
                    let is_complete = normalized_line.is_empty();
                    response_lines.push(normalized_line);
                    if is_complete {
                        break;
                    }
                }
                Err(_) => return Err(GtpError::UnexpectedEndOfStream),
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

    pub fn stop(&mut self) -> Result<(), std::io::Error> {
        self.child.kill()?;
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        Ok(())
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
}

impl AnalysisStream {
    pub fn start(executable: &str, arguments: &[String]) -> Result<Self, GtpError> {
        let mut child = Command::new(executable)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let standard_input = child.stdin.take().ok_or(GtpError::MissingStandardInput)?;
        let standard_output = child.stdout.take().ok_or(GtpError::MissingStandardOutput)?;
        let (sender, receiver) = std::sync::mpsc::channel();
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
        });
        Ok(Self {
            child,
            standard_input,
            receiver,
            reader_thread: Some(reader_thread),
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

    /// Returns the next line without blocking, or `None` when nothing is
    /// buffered yet.
    pub fn next_line(&mut self) -> Option<String> {
        self.receiver.try_recv().ok()
    }

    /// Waits up to `timeout` for the next line.
    pub fn recv_line_timeout(&mut self, timeout: std::time::Duration) -> Option<String> {
        self.receiver.recv_timeout(timeout).ok()
    }

    /// Asks a searching engine to stop and emit its final analysis.
    pub fn stop(&mut self) -> std::io::Result<()> {
        self.send_command("stop")
    }

    /// Kills the process and joins the reader thread.
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()?;
        self.join_reader();
        Ok(())
    }

    fn join_reader(&mut self) {
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AnalysisStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.join_reader();
    }
}

#[cfg(test)]
mod stream_tests {
    use super::AnalysisStream;
    use std::time::Duration;

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
}
