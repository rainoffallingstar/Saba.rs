use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
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
    standard_output: BufReader<ChildStdout>,
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
        let standard_output = child
            .stdout
            .take()
            .map(BufReader::new)
            .ok_or(GtpError::MissingStandardOutput)?;
        Ok(Self {
            child,
            standard_input,
            standard_output,
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
            let mut line = String::new();
            let bytes_read = self.standard_output.read_line(&mut line)?;
            if bytes_read == 0 {
                return Err(GtpError::UnexpectedEndOfStream);
            }
            let normalized_line = line.trim_end_matches(['\r', '\n']).to_owned();
            let is_complete = normalized_line.is_empty();
            response_lines.push(normalized_line);
            if is_complete {
                break;
            }
        }
        parse_response(response_lines)
    }

    pub fn stop(&mut self) -> Result<(), std::io::Error> {
        self.child.kill()
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
