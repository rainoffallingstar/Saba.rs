use std::collections::BTreeSet;

use sabaki_domain_core::gtp::{GtpError, GtpProcessSupervisor, GtpResponse};
use thiserror::Error;

use crate::engine_workflow::EngineRecord;

/// Process transport boundary for a GTP engine. The real implementation wraps
/// `GtpProcessSupervisor`; tests inject an in-memory responder so the session
/// lifecycle can be exercised without a bundled engine binary.
pub trait GtpTransport {
    fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError>;

    /// Writes a streaming command without waiting for a complete response
    /// (e.g. `kata-analyze`); output is read with `recv_line_timeout` until
    /// `stop` is sent. Transports that cannot stream return
    /// `GtpError::UnsupportedStreaming`.
    fn send_streaming(&mut self, _name: &str, _arguments: Vec<String>) -> Result<(), GtpError> {
        Err(GtpError::UnsupportedStreaming)
    }

    /// Asks a running streaming analysis to stop without consuming its
    /// in-flight output records. Defaults to `send_streaming("stop")` so
    /// bounded-command channel stealing can never corrupt the stream tail.
    fn stop_streaming(&mut self) -> Result<(), GtpError> {
        self.send_streaming("stop", Vec::new())
    }

    /// Waits up to `timeout` for the next line of a streaming command.
    fn recv_line_timeout(&mut self, _timeout: std::time::Duration) -> Option<String> {
        None
    }

    /// True once the engine's output stream closed (the engine exited).
    fn is_stream_closed(&self) -> bool {
        false
    }

    /// Trailing engine stderr lines, for diagnosing abrupt exits.
    fn stderr_tail(&self) -> String {
        String::new()
    }

    fn stop(&mut self) -> Result<(), std::io::Error>;
}

/// Production transport backed by a real engine subprocess.
pub struct ProcessGtpTransport {
    supervisor: GtpProcessSupervisor,
}

impl ProcessGtpTransport {
    pub fn start(executable: &str, arguments: &[String]) -> Result<Self, GtpError> {
        Ok(Self {
            supervisor: GtpProcessSupervisor::start(executable, arguments)?,
        })
    }
}

impl GtpTransport for ProcessGtpTransport {
    fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError> {
        self.supervisor.send(name, arguments)
    }

    fn send_streaming(&mut self, name: &str, arguments: Vec<String>) -> Result<(), GtpError> {
        self.supervisor.send_streaming(name, arguments)
    }

    fn stop_streaming(&mut self) -> Result<(), GtpError> {
        self.supervisor.send_streaming("stop", Vec::new())
    }

    fn recv_line_timeout(&mut self, timeout: std::time::Duration) -> Option<String> {
        self.supervisor.recv_line_timeout(timeout)
    }

    fn is_stream_closed(&self) -> bool {
        self.supervisor.is_stream_closed()
    }

    fn stderr_tail(&self) -> String {
        self.supervisor.stderr_tail()
    }

    fn stop(&mut self) -> Result<(), std::io::Error> {
        self.supervisor.stop()
    }
}

#[derive(Debug, Error)]
pub enum EngineSessionError {
    #[error(transparent)]
    Transport(#[from] GtpError),
    #[error("engine rejected the {0} handshake: {1}")]
    Handshake(String, String),
    #[error("engine did not report a version")]
    MissingVersion,
}

/// Lifecycle state of an engine session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineSessionState {
    Ready { name: String, version: String },
    Stopped,
}

/// A UI-independent GTP engine session: startup handshake, capability probe,
/// optional startup commands, board setup, play/generate, raw command
/// forwarding and shutdown. Sessions are always driven through an injected
/// transport so host tests stay hermetic.
#[derive(Debug)]
pub struct EngineSession<T: GtpTransport> {
    transport: T,
    state: EngineSessionState,
    board_size: usize,
    capabilities: BTreeSet<String>,
}

impl<T: GtpTransport> EngineSession<T> {
    /// Starts the engine, performs the protocol handshake, probes the command
    /// set, runs any startup commands from the record and prepares the given
    /// board size.
    pub fn start(
        transport: T,
        record: &EngineRecord,
        board_size: usize,
    ) -> Result<Self, EngineSessionError> {
        let mut session = Self {
            transport,
            state: EngineSessionState::Stopped,
            board_size,
            capabilities: BTreeSet::new(),
        };
        let name = session.handshake_name()?;
        let version = session.handshake_version()?;
        session.run_startup_commands(record.commands.as_deref())?;
        session.configure_board()?;
        session.probe_capabilities();
        session.state = EngineSessionState::Ready { name, version };
        Ok(session)
    }

    fn handshake_name(&mut self) -> Result<String, EngineSessionError> {
        let response = self.transport.send("name", Vec::new())?;
        if response.success {
            Ok(response.content)
        } else {
            Err(EngineSessionError::Handshake(
                "name".to_owned(),
                response.content,
            ))
        }
    }

    fn handshake_version(&mut self) -> Result<String, EngineSessionError> {
        let response = self.transport.send("version", Vec::new())?;
        if !response.success {
            return Err(EngineSessionError::Handshake(
                "version".to_owned(),
                response.content,
            ));
        }
        if response.content.trim().is_empty() {
            return Err(EngineSessionError::MissingVersion);
        }
        Ok(response.content)
    }

    /// Collects the engine's advertised command set from `list_commands`.
    /// Engines that reject or omit the probe simply keep an empty set; the
    /// probe never fails session startup.
    fn probe_capabilities(&mut self) {
        if let Ok(response) = self.transport.send("list_commands", Vec::new())
            && response.success
        {
            self.capabilities = response
                .content
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect();
        }
    }

    fn run_startup_commands(&mut self, commands: Option<&str>) -> Result<(), EngineSessionError> {
        let Some(commands) = commands else {
            return Ok(());
        };
        for line in commands.lines().filter(|line| !line.trim().is_empty()) {
            let mut tokens = line.split_whitespace();
            let command_name = tokens.next().unwrap_or_default();
            let arguments: Vec<String> = tokens.map(ToOwned::to_owned).collect();
            let response = self.transport.send(command_name, arguments)?;
            if !response.success {
                return Err(EngineSessionError::Handshake(
                    command_name.to_owned(),
                    response.content,
                ));
            }
        }
        Ok(())
    }

    fn configure_board(&mut self) -> Result<(), EngineSessionError> {
        let boardsize = self
            .transport
            .send("boardsize", vec![self.board_size.to_string()])?;
        if !boardsize.success {
            return Err(EngineSessionError::Handshake(
                "boardsize".to_owned(),
                boardsize.content,
            ));
        }
        let clear = self.transport.send("clear_board", Vec::new())?;
        if !clear.success {
            return Err(EngineSessionError::Handshake(
                "clear_board".to_owned(),
                clear.content,
            ));
        }
        Ok(())
    }

    pub fn state(&self) -> &EngineSessionState {
        &self.state
    }

    /// The command set advertised by the engine's `list_commands` probe. An
    /// empty set means the probe was rejected or omitted.
    pub fn capabilities(&self) -> &BTreeSet<String> {
        &self.capabilities
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn board_size(&self) -> usize {
        self.board_size
    }

    /// Forwards an arbitrary GTP command to the engine without interpreting
    /// its response, for console use.
    pub fn send_command(
        &mut self,
        name: &str,
        arguments: Vec<String>,
    ) -> Result<GtpResponse, GtpError> {
        self.transport.send(name, arguments)
    }

    pub fn play(&mut self, color: &str, vertex: &str) -> Result<GtpResponse, GtpError> {
        self.transport
            .send("play", vec![color.to_owned(), vertex.to_owned()])
    }

    pub fn generate_move(&mut self, color: &str) -> Result<GtpResponse, GtpError> {
        self.transport.send("genmove", vec![color.to_owned()])
    }

    /// Sends an analysis command (`analyze`, `lz-analyze`, `kata-analyze`,
    /// ...) and parses the engine's response into structured entries. The
    /// transport framing stays request/response; streaming search updates
    /// arrive as one bounded response per request.
    pub fn analyze(
        &mut self,
        command: &str,
        arguments: Vec<String>,
    ) -> Result<Vec<crate::AnalysisEntry>, GtpError> {
        let response = self.transport.send(command, arguments)?;
        if response.success {
            Ok(crate::parse_analysis_response(command, &response.content))
        } else {
            Ok(Vec::new())
        }
    }

    /// Asks a searching engine to stop and return its current analysis.
    /// Bounded GTP `stop`; safe to run mid-stream because the supervisor now
    /// skips in-flight streaming records when collecting the response.
    pub fn stop_analysis(&mut self) -> Result<GtpResponse, GtpError> {
        self.transport.send("stop", Vec::new())
    }

    /// Asks a streaming search to stop without consuming its tail records.
    pub fn stop_streaming(&mut self) -> Result<(), GtpError> {
        self.transport.stop_streaming()
    }

    /// True once the engine's output stream closed (engine exited).
    pub fn is_stream_closed(&self) -> bool {
        self.transport.is_stream_closed()
    }

    /// Trailing engine stderr lines for diagnosing abrupt exits.
    pub fn stderr_tail(&self) -> String {
        self.transport.stderr_tail()
    }

    /// Starts a streaming analysis on the already-connected session (no new
    /// process). Fails with `GtpError::UnsupportedStreaming` when the
    /// transport cannot stream; callers then fall back to a fresh
    /// `AnalysisStream` process.
    pub fn stream_analyze(
        &mut self,
        command: &str,
        arguments: Vec<String>,
    ) -> Result<(), GtpError> {
        self.transport.send_streaming(command, arguments)
    }

    /// Reads the next line of the running streaming analysis with a timeout.
    pub fn recv_analysis_line(&mut self, timeout: std::time::Duration) -> Option<String> {
        self.transport.recv_line_timeout(timeout)
    }

    pub fn stop(&mut self) -> Result<(), std::io::Error> {
        self.transport.stop()?;
        self.state = EngineSessionState::Stopped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineSession, EngineSessionError, EngineSessionState, GtpTransport};
    use crate::engine_workflow::EngineRecord;
    use sabaki_domain_core::gtp::{GtpError, GtpResponse};
    use std::{cell::RefCell, collections::BTreeMap, time::Duration};

    #[derive(Default, Debug)]
    struct MockTransport {
        responses: RefCell<BTreeMap<String, GtpResponse>>,
        calls: RefCell<Vec<String>>,
        /// Streaming lines consumed by `recv_line_timeout`.
        stream_lines: RefCell<std::collections::VecDeque<String>>,
        /// When true, `send_streaming` fails with `UnsupportedStreaming`.
        streaming_unsupported: bool,
    }

    impl MockTransport {
        fn responding(entries: &[(&str, bool, &str)]) -> Self {
            let responses = entries
                .iter()
                .map(|(name, success, content)| {
                    (
                        (*name).to_owned(),
                        GtpResponse {
                            identifier: None,
                            success: *success,
                            content: (*content).to_owned(),
                        },
                    )
                })
                .collect();
            Self {
                responses: RefCell::new(responses),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl GtpTransport for MockTransport {
        fn send(&mut self, name: &str, _arguments: Vec<String>) -> Result<GtpResponse, GtpError> {
            self.calls.borrow_mut().push(name.to_owned());
            Ok(self
                .responses
                .borrow()
                .get(name)
                .cloned()
                .unwrap_or_else(|| GtpResponse {
                    identifier: None,
                    success: true,
                    content: String::new(),
                }))
        }

        fn send_streaming(&mut self, name: &str, _arguments: Vec<String>) -> Result<(), GtpError> {
            self.calls.borrow_mut().push(name.to_owned());
            if self.streaming_unsupported {
                return Err(GtpError::UnsupportedStreaming);
            }
            Ok(())
        }

        fn recv_line_timeout(&mut self, _timeout: std::time::Duration) -> Option<String> {
            self.stream_lines.borrow_mut().pop_front()
        }

        fn stop(&mut self) -> Result<(), std::io::Error> {
            Ok(())
        }
    }

    fn record_with_commands(commands: Option<&str>) -> EngineRecord {
        let mut record = EngineRecord::new("KataGo", "/engines/katago", "");
        if let Some(commands) = commands {
            record.commands = Some(commands.to_owned());
        }
        record
    }

    #[test]
    fn streaming_analysis_reuses_the_connected_session() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("list_commands", true, "play\ngenmove\nkata-analyze"),
        ]);
        let record = record_with_commands(None);
        let mut session = EngineSession::start(transport, &record, 19).expect("session starts");

        session
            .stream_analyze("kata-analyze", Vec::new())
            .expect("streaming command is sent");
        assert_eq!(
            session.transport().calls().last().map(String::as_str),
            Some("kata-analyze"),
            "the streaming command must go through the connected session, not a new process"
        );
        assert_eq!(
            session.recv_analysis_line(Duration::from_millis(50)),
            None,
            "the mock has no streamed lines yet"
        );
        session.stop().expect("session stops cleanly");
    }

    #[test]
    fn streaming_analysis_reports_unsupported_transports() {
        let mut transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("list_commands", true, "play\ngenmove\nkata-analyze"),
        ]);
        transport.streaming_unsupported = true;
        let record = record_with_commands(None);
        let mut session = EngineSession::start(transport, &record, 19).expect("session starts");

        assert!(matches!(
            session.stream_analyze("kata-analyze", Vec::new()),
            Err(GtpError::UnsupportedStreaming)
        ));
        session.stop().expect("session stops cleanly");
    }

    #[test]
    fn handshake_configure_board_and_startup_commands_order_is_stable() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("level", true, ""),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("list_commands", true, "play\ngenmove\nkata-analyze"),
        ]);
        let record = record_with_commands(Some("level 10\nkomi 6.5"));

        let session = EngineSession::start(transport, &record, 19).expect("session starts");

        assert_eq!(
            session.state(),
            &EngineSessionState::Ready {
                name: "KataGo".to_owned(),
                version: "1.16.4".to_owned(),
            }
        );
        assert_eq!(session.board_size(), 19);
        assert_eq!(
            session.transport().calls(),
            vec![
                "name".to_owned(),
                "version".to_owned(),
                "level".to_owned(),
                "komi".to_owned(),
                "boardsize".to_owned(),
                "clear_board".to_owned(),
                "list_commands".to_owned(),
            ]
        );
        assert!(session.capabilities().contains("kata-analyze"));
        assert!(session.capabilities().contains("genmove"));
        assert_eq!(session.capabilities().len(), 3);
    }

    #[test]
    fn an_empty_version_reports_missing_version() {
        let transport =
            MockTransport::responding(&[("name", true, "KataGo"), ("version", true, "")]);

        let error = EngineSession::start(transport, &record_with_commands(None), 19)
            .expect_err("an empty version fails startup");

        assert!(matches!(error, EngineSessionError::MissingVersion));
    }

    #[test]
    fn a_rejected_capability_probe_keeps_an_empty_command_set() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("list_commands", false, "not supported"),
        ]);

        let session = EngineSession::start(transport, &record_with_commands(None), 19)
            .expect("session starts");

        assert!(session.capabilities().is_empty());
    }

    #[test]
    fn a_failed_handshake_surfaces_the_rejection() {
        let transport = MockTransport::responding(&[("name", false, "not authorized")]);
        let record = record_with_commands(None);

        let error = EngineSession::start(transport, &record, 19)
            .expect_err("a rejected handshake fails startup");

        assert!(matches!(
            error,
            EngineSessionError::Handshake(command, _) if command == "name"
        ));
    }

    #[test]
    fn play_and_generate_move_forward_to_the_transport() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
        ]);
        let record = record_with_commands(None);
        let mut session = EngineSession::start(transport, &record, 19).expect("session starts");

        session.play("B", "D4").expect("play succeeds");
        session.generate_move("W").expect("genmove succeeds");

        let calls = session.transport().calls();
        assert!(calls.contains(&"play".to_owned()));
        assert!(calls.contains(&"genmove".to_owned()));
    }

    #[test]
    fn send_command_forwards_arbitrary_console_commands() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("time_settings", true, ""),
        ]);
        let record = record_with_commands(None);
        let mut session = EngineSession::start(transport, &record, 19).expect("session starts");

        let response = session
            .send_command("time_settings", vec!["600".to_owned(), "10".to_owned()])
            .expect("console commands forward");

        assert!(response.success);
        assert!(
            session
                .transport()
                .calls()
                .contains(&"time_settings".to_owned())
        );
    }

    #[test]
    fn stopping_marks_the_session_stopped() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
        ]);
        let record = record_with_commands(None);
        let mut session = EngineSession::start(transport, &record, 19).expect("session starts");

        session.stop().expect("engine stops");
        assert_eq!(session.state(), &EngineSessionState::Stopped);
    }
}
