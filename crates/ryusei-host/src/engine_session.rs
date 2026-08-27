use std::{collections::BTreeSet, time::Duration};

use ryusei_domain_core::gtp::{
    DEFAULT_COMMAND_TIMEOUT, GtpError, GtpProcessSupervisor, GtpResponse, SEARCH_COMMAND_TIMEOUT,
};
use ryusei_domain_core::{ClockState, Color, TimeControl};
use thiserror::Error;

use crate::engine_workflow::EngineRecord;

/// Process transport boundary for a GTP engine. The real implementation wraps
/// `GtpProcessSupervisor`; tests inject an in-memory responder so the session
/// lifecycle can be exercised without a bundled engine binary.
pub trait GtpTransport {
    fn send_with_timeout(
        &mut self,
        name: &str,
        arguments: Vec<String>,
        timeout: Duration,
    ) -> Result<GtpResponse, GtpError>;

    fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError> {
        self.send_with_timeout(name, arguments, DEFAULT_COMMAND_TIMEOUT)
    }

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
        Self::start_in(executable, arguments, None)
    }

    /// Starts the engine in an explicit working directory. KataGo's generated
    /// config writes a relative `logDir`; a non-writable cwd (packaged app)
    /// makes it abort during startup, which the UI surfaces as a handshake
    /// failure. The app passes its writable config directory here.
    pub fn start_in(
        executable: &str,
        arguments: &[String],
        current_dir: Option<&std::path::Path>,
    ) -> Result<Self, GtpError> {
        Ok(Self {
            supervisor: GtpProcessSupervisor::start_in(executable, arguments, current_dir)?,
        })
    }
}

impl GtpTransport for ProcessGtpTransport {
    fn send_with_timeout(
        &mut self,
        name: &str,
        arguments: Vec<String>,
        timeout: Duration,
    ) -> Result<GtpResponse, GtpError> {
        self.supervisor.send_with_timeout(name, arguments, timeout)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineCommandTimeouts {
    pub ordinary: Duration,
    pub search: Duration,
    pub stop: Duration,
}

impl Default for EngineCommandTimeouts {
    fn default() -> Self {
        Self {
            ordinary: DEFAULT_COMMAND_TIMEOUT,
            search: SEARCH_COMMAND_TIMEOUT,
            stop: DEFAULT_COMMAND_TIMEOUT,
        }
    }
}

#[derive(Debug, Error)]
pub enum EngineSessionError {
    #[error(transparent)]
    Transport(#[from] GtpError),
    #[error("engine rejected the {0} handshake: {1}")]
    Handshake(String, String),
    #[error("engine rejected `{command}`: {content}")]
    CommandRejected { command: String, content: String },
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
    timeouts: EngineCommandTimeouts,
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
        Self::start_with_timeouts(
            transport,
            record,
            board_size,
            EngineCommandTimeouts::default(),
        )
    }

    pub fn start_with_timeouts(
        transport: T,
        record: &EngineRecord,
        board_size: usize,
        timeouts: EngineCommandTimeouts,
    ) -> Result<Self, EngineSessionError> {
        let mut session = Self {
            transport,
            state: EngineSessionState::Stopped,
            board_size,
            capabilities: BTreeSet::new(),
            timeouts,
        };
        let name = session.handshake_name()?;
        let version = session.handshake_version()?;
        session.run_startup_commands(record.commands.as_deref())?;
        session.configure_board()?;
        session.probe_capabilities();
        session.state = EngineSessionState::Ready { name, version };
        Ok(session)
    }

    fn send_ordinary(
        &mut self,
        name: &str,
        arguments: Vec<String>,
    ) -> Result<GtpResponse, GtpError> {
        self.transport
            .send_with_timeout(name, arguments, self.timeouts.ordinary)
    }

    fn handshake_name(&mut self) -> Result<String, EngineSessionError> {
        let response = self.send_ordinary("name", Vec::new())?;
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
        let response = self.send_ordinary("version", Vec::new())?;
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
        if let Ok(response) = self.send_ordinary("list_commands", Vec::new())
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
            let response = self.send_ordinary(command_name, arguments)?;
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
        let boardsize = self.send_ordinary("boardsize", vec![self.board_size.to_string()])?;
        if !boardsize.success {
            return Err(EngineSessionError::Handshake(
                "boardsize".to_owned(),
                boardsize.content,
            ));
        }
        let clear = self.send_ordinary("clear_board", Vec::new())?;
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
        self.send_ordinary(name, arguments)
    }

    pub fn play(&mut self, color: &str, vertex: &str) -> Result<GtpResponse, GtpError> {
        self.send_ordinary("play", vec![color.to_owned(), vertex.to_owned()])
    }

    pub fn generate_move(&mut self, color: &str) -> Result<GtpResponse, GtpError> {
        self.transport
            .send_with_timeout("genmove", vec![color.to_owned()], self.timeouts.search)
    }

    /// Configures the engine's clock before a local or remote game starts.
    /// Applies SGF rule, komi, and handicap-independent game settings before
    /// the position is replayed. The JSON Ancient Chinese ruleset is passed as
    /// one GTP argument, not split on whitespace.
    pub fn set_game_rules(
        &mut self,
        config: &crate::GameRuleConfig,
    ) -> Result<(), EngineSessionError> {
        let rules = self.send_ordinary(
            "kata-set-rules",
            vec![config.ruleset.katago_name().to_owned()],
        )?;
        if !rules.success {
            return Err(EngineSessionError::Handshake(
                "kata-set-rules".to_owned(),
                rules.content,
            ));
        }
        let komi = self.send_ordinary("komi", vec![format!("{:.1}", config.komi)])?;
        if !komi.success {
            return Err(EngineSessionError::Handshake(
                "komi".to_owned(),
                komi.content,
            ));
        }
        Ok(())
    }

    pub fn set_time_control(&mut self, control: TimeControl) -> Result<GtpResponse, GtpError> {
        let arguments = match control {
            TimeControl::None => vec!["0".to_owned(), "0".to_owned(), "0".to_owned()],
            TimeControl::Absolute { main_time_secs } => {
                vec![main_time_secs.to_string(), "0".to_owned(), "0".to_owned()]
            }
            TimeControl::ByoYomi {
                main_time_secs,
                period_time_secs,
                periods,
            } => vec![
                main_time_secs.to_string(),
                period_time_secs.to_string(),
                periods.to_string(),
            ],
        };
        self.send_ordinary("time_settings", arguments)
    }

    /// Synchronizes a local predictive clock to a GTP engine before `genmove`.
    /// Remote adapters must pass their server-authoritative clock state here.
    pub fn sync_clock_state(&mut self, state: ClockState) -> Result<(), EngineSessionError> {
        if matches!(state.control, TimeControl::None) {
            return Ok(());
        }
        let settings = self.set_time_control(state.control)?;
        if !settings.success {
            return Err(EngineSessionError::CommandRejected {
                command: "time_settings".to_owned(),
                content: settings.content,
            });
        }
        for color in [Color::Black, Color::White] {
            let player = state.player(color);
            let response =
                self.set_time_left(color, player.display_remaining(), player.periods_remaining)?;
            if !response.success {
                return Err(EngineSessionError::CommandRejected {
                    command: "time_left".to_owned(),
                    content: response.content,
                });
            }
        }
        Ok(())
    }

    /// Reports the current remaining time for one color to the engine.
    pub fn set_time_left(
        &mut self,
        color: Color,
        remaining: std::time::Duration,
        periods: u32,
    ) -> Result<GtpResponse, GtpError> {
        self.send_ordinary(
            "time_left",
            vec![
                match color {
                    Color::Black => "B".to_owned(),
                    Color::White => "W".to_owned(),
                },
                format!("{:.3}", remaining.as_secs_f64()),
                periods.to_string(),
            ],
        )
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
        let response =
            self.transport
                .send_with_timeout(command, arguments, self.timeouts.search)?;
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
        self.transport
            .send_with_timeout("stop", Vec::new(), self.timeouts.stop)
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
        self.state = EngineSessionState::Stopped;
        self.transport.stop()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EngineCommandTimeouts, EngineSession, EngineSessionError, EngineSessionState, GtpTransport,
    };
    use crate::engine_workflow::EngineRecord;
    use ryusei_domain_core::gtp::{GtpError, GtpResponse};
    use ryusei_domain_core::{ClockState, Color, TimeControl};
    use std::{cell::RefCell, collections::BTreeMap, time::Duration};

    #[derive(Default, Debug)]
    struct MockTransport {
        responses: RefCell<BTreeMap<String, GtpResponse>>,
        calls: RefCell<Vec<String>>,
        calls_with_arguments: RefCell<Vec<(String, Vec<String>)>>,
        timed_calls: RefCell<Vec<(String, Duration)>>,
        /// Streaming lines consumed by `recv_line_timeout`.
        stream_lines: RefCell<std::collections::VecDeque<String>>,
        /// When true, `send_streaming` fails with `UnsupportedStreaming`.
        streaming_unsupported: bool,
        stop_fails: bool,
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

        fn calls_with_arguments(&self) -> Vec<(String, Vec<String>)> {
            self.calls_with_arguments.borrow().clone()
        }
    }

    impl GtpTransport for MockTransport {
        fn send_with_timeout(
            &mut self,
            name: &str,
            arguments: Vec<String>,
            timeout: Duration,
        ) -> Result<GtpResponse, GtpError> {
            self.calls.borrow_mut().push(name.to_owned());
            self.calls_with_arguments
                .borrow_mut()
                .push((name.to_owned(), arguments));
            self.timed_calls
                .borrow_mut()
                .push((name.to_owned(), timeout));
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
            if self.stop_fails {
                Err(std::io::Error::other("fixture cleanup failed"))
            } else {
                Ok(())
            }
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
    fn command_classes_use_the_configured_timeout_policy() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("genmove", true, "D4"),
            (
                "kata-analyze",
                true,
                "info move D4 visits 1 winrate 0.5 pv D4",
            ),
            ("stop", true, ""),
        ]);
        let timeouts = EngineCommandTimeouts {
            ordinary: Duration::from_millis(11),
            search: Duration::from_millis(22),
            stop: Duration::from_millis(33),
        };
        let mut session = EngineSession::start_with_timeouts(
            transport,
            &record_with_commands(None),
            19,
            timeouts,
        )
        .expect("session starts");

        session.generate_move("B").expect("search succeeds");
        session
            .analyze("kata-analyze", Vec::new())
            .expect("bounded analysis succeeds");
        session.stop_analysis().expect("analysis stops");

        let calls = session.transport().timed_calls.borrow();
        assert!(
            calls
                .iter()
                .any(|(name, timeout)| name == "name" && *timeout == timeouts.ordinary)
        );
        assert!(
            calls
                .iter()
                .any(|(name, timeout)| name == "genmove" && *timeout == timeouts.search)
        );
        assert!(
            calls
                .iter()
                .any(|(name, timeout)| name == "kata-analyze" && *timeout == timeouts.search)
        );
        assert!(
            calls
                .iter()
                .any(|(name, timeout)| name == "stop" && *timeout == timeouts.stop)
        );
    }

    #[test]
    fn typed_time_commands_forward_standard_gtp_arguments() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
        ]);
        let mut session = EngineSession::start(transport, &record_with_commands(None), 19)
            .expect("session starts");
        session
            .set_time_control(TimeControl::ByoYomi {
                main_time_secs: 600,
                period_time_secs: 30,
                periods: 5,
            })
            .expect("time settings succeeds");
        session
            .set_time_left(Color::White, Duration::from_millis(12_345), 4)
            .expect("time left succeeds");
        let calls = session.transport().calls_with_arguments();
        assert!(
            calls
                .iter()
                .any(|(name, args)| { name == "time_settings" && args == &["600", "30", "5"] })
        );
        assert!(
            calls
                .iter()
                .any(|(name, args)| { name == "time_left" && args == &["W", "12.345", "4"] })
        );
    }

    #[test]
    fn clock_sync_reports_both_player_clocks_before_genmove() {
        let transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
            ("time_settings", true, ""),
            ("time_left", true, ""),
        ]);
        let mut session = EngineSession::start(transport, &record_with_commands(None), 19)
            .expect("session starts");
        let state = ClockState::new(TimeControl::ByoYomi {
            main_time_secs: 600,
            period_time_secs: 30,
            periods: 5,
        });

        session
            .sync_clock_state(state)
            .expect("clock sync succeeds");

        let calls = session.transport().calls_with_arguments();
        assert!(
            calls
                .iter()
                .any(|(name, args)| { name == "time_settings" && args == &vec!["600", "30", "5"] })
        );
        assert!(
            calls
                .iter()
                .any(|(name, args)| { name == "time_left" && args == &vec!["B", "600.000", "5"] })
        );
        assert!(
            calls
                .iter()
                .any(|(name, args)| { name == "time_left" && args == &vec!["W", "600.000", "5"] })
        );
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
    fn failed_cleanup_still_makes_the_session_terminal() {
        let mut transport = MockTransport::responding(&[
            ("name", true, "KataGo"),
            ("version", true, "1.16.4"),
            ("boardsize", true, ""),
            ("clear_board", true, ""),
        ]);
        transport.stop_fails = true;
        let mut session = EngineSession::start(transport, &record_with_commands(None), 19)
            .expect("session starts");

        assert!(session.stop().is_err());
        assert_eq!(session.state(), &EngineSessionState::Stopped);
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
