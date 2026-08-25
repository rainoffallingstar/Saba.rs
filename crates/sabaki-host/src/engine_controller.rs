//! Connected-engine lifecycle Module.
//!
//! `EngineController` owns the role-to-session map. Its Interface describes
//! game-level operations (attach to a position, send, generate, analyze,
//! synchronize, and lease a streaming session); handshake, board replay, GTP
//! vertex formatting and shutdown remain implementation detail.

use std::{collections::BTreeMap, time::Duration};

use sabaki_domain_core::gtp::{GtpError, GtpResponse};
use sabaki_domain_core::{Color, MoveDto};

use crate::{EngineRecord, EngineSession, EngineSessionError, GtpTransport};

/// A role-keyed deep Module for connected GTP sessions. `R` belongs to the
/// presentation/domain policy; the controller only needs stable ordering.
pub struct EngineController<R, T: GtpTransport> {
    sessions: BTreeMap<R, EngineSession<T>>,
}

impl<R, T: GtpTransport> Default for EngineController<R, T> {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
        }
    }
}

impl<R: Copy + Ord, T: GtpTransport> EngineController<R, T> {
    /// Handshakes a new transport and replays the complete position before the
    /// session becomes visible. A failed replay stops the new transport and
    /// leaves the existing role map untouched.
    pub fn attach(
        &mut self,
        role: R,
        transport: T,
        record: &EngineRecord,
        board_size: usize,
        moves: &[MoveDto],
    ) -> Result<(), EngineControllerError> {
        if self.sessions.contains_key(&role) {
            return Err(EngineControllerError::AlreadyAttached);
        }
        let mut session = EngineSession::start(transport, record, board_size)?;
        if let Err(error) = replay_position(&mut session, board_size, moves) {
            session.stop().ok();
            return Err(EngineControllerError::Transport(error));
        }
        self.sessions.insert(role, session);
        Ok(())
    }

    pub fn is_attached(&self, role: R) -> bool {
        self.sessions.contains_key(&role)
    }

    pub fn any_attached(&self) -> bool {
        !self.sessions.is_empty()
    }

    /// Detaches one role and best-effort stops its transport.
    pub fn detach(&mut self, role: R) -> bool {
        if let Some(mut session) = self.sessions.remove(&role) {
            session.stop().ok();
            true
        } else {
            false
        }
    }

    /// Stops and clears every attached transport.
    pub fn detach_all(&mut self) {
        for (_, mut session) in std::mem::take(&mut self.sessions) {
            session.stop().ok();
        }
    }

    /// Sends a raw GTP console command to a connected role.
    pub fn send(
        &mut self,
        role: R,
        name: &str,
        arguments: Vec<String>,
    ) -> Result<GtpResponse, EngineControllerError> {
        self.session_mut(role)?
            .send_command(name, arguments)
            .map_err(Into::into)
    }

    /// Requests one move from a connected role.
    pub fn request_move(
        &mut self,
        role: R,
        color: Color,
    ) -> Result<GtpResponse, EngineControllerError> {
        let color = match color {
            Color::Black => "B",
            Color::White => "W",
        };
        self.session_mut(role)?
            .generate_move(color)
            .map_err(Into::into)
    }

    /// Runs a bounded analysis command on the connected session.
    pub fn analyze(
        &mut self,
        role: R,
        command: &str,
        arguments: Vec<String>,
    ) -> Result<Vec<crate::AnalysisEntry>, EngineControllerError> {
        self.session_mut(role)?
            .analyze(command, arguments)
            .map_err(Into::into)
    }

    /// Replays a full position into an attached role. Used before streaming and
    /// when a leased analysis session returns after local moves.
    pub fn replay(
        &mut self,
        role: R,
        board_size: usize,
        moves: &[MoveDto],
    ) -> Result<(), EngineControllerError> {
        replay_position(self.session_mut(role)?, board_size, moves).map_err(Into::into)
    }

    /// Starts streaming analysis on an attached role.
    pub fn start_analysis(
        &mut self,
        role: R,
        command: &str,
        arguments: Vec<String>,
    ) -> Result<(), EngineControllerError> {
        self.session_mut(role)?
            .stream_analyze(command, arguments)
            .map_err(Into::into)
    }

    /// Broadcasts a move to every idle attached role except its source. Each
    /// error is returned with its role so shells can present context without
    /// traversing or mutating the hidden session map.
    pub fn synchronize_move(
        &mut self,
        source: Option<R>,
        color: Color,
        vertex: Option<(usize, usize)>,
    ) -> Vec<(R, GtpError)> {
        let color = match color {
            Color::Black => "B",
            Color::White => "W",
        };
        let mut errors = Vec::new();
        for (role, session) in &mut self.sessions {
            if Some(*role) == source {
                continue;
            }
            let vertex = vertex
                .map(|(column, row)| format_gtp_vertex(session.board_size(), column, row))
                .unwrap_or_else(|| "pass".to_owned());
            if let Err(error) = session.play(color, &vertex) {
                errors.push((*role, error));
            }
        }
        errors
    }

    /// Takes exclusive ownership of a session for an async streaming worker.
    /// While leased, the role is detached from all other Interface operations.
    pub fn lease_for_analysis(
        &mut self,
        role: R,
    ) -> Result<EngineSession<T>, EngineControllerError> {
        self.sessions
            .remove(&role)
            .ok_or(EngineControllerError::Detached)
    }

    /// Returns a previously leased session to its role. An existing session is
    /// replaced and stopped first, preserving one-session-per-role invariant.
    pub fn return_analysis_lease(&mut self, role: R, session: EngineSession<T>) {
        if let Some(mut previous) = self.sessions.insert(role, session) {
            previous.stop().ok();
        }
    }

    /// Replays an attached stream lease without exposing the entire session map.
    pub fn replay_leased(
        session: &mut EngineSession<T>,
        board_size: usize,
        moves: &[MoveDto],
    ) -> Result<(), EngineControllerError> {
        replay_position(session, board_size, moves).map_err(Into::into)
    }

    /// Reads one line from a leased streaming session.
    pub fn recv_analysis_line(session: &mut EngineSession<T>, timeout: Duration) -> Option<String> {
        session.recv_analysis_line(timeout)
    }

    /// Stops a leased streaming session's search without consuming its tail.
    pub fn stop_leased_analysis(
        session: &mut EngineSession<T>,
    ) -> Result<(), EngineControllerError> {
        session.stop_streaming().map_err(Into::into)
    }

    /// True once the leased session's engine output stream closed.
    pub fn session_stream_closed(session: &EngineSession<T>) -> bool {
        session.is_stream_closed()
    }

    /// Trailing engine stderr lines for diagnosing abrupt exits.
    pub fn session_stderr_tail(session: &EngineSession<T>) -> String {
        session.stderr_tail()
    }

    fn session_mut(&mut self, role: R) -> Result<&mut EngineSession<T>, EngineControllerError> {
        self.sessions
            .get_mut(&role)
            .ok_or(EngineControllerError::Detached)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineControllerError {
    #[error("engine role is already attached")]
    AlreadyAttached,
    #[error("engine role is detached")]
    Detached,
    #[error(transparent)]
    Session(#[from] EngineSessionError),
    #[error(transparent)]
    Transport(#[from] GtpError),
}

fn replay_position<T: GtpTransport>(
    session: &mut EngineSession<T>,
    board_size: usize,
    moves: &[MoveDto],
) -> Result<(), GtpError> {
    session.send_command("boardsize", vec![board_size.to_string()])?;
    session.send_command("clear_board", Vec::new())?;
    for move_dto in moves {
        let color = match move_dto.color {
            Color::Black => "B",
            Color::White => "W",
        };
        let vertex = move_dto
            .vertex
            .map(|vertex| format_gtp_vertex(board_size, vertex.column, vertex.row))
            .unwrap_or_else(|| "pass".to_owned());
        session.play(color, &vertex)?;
    }
    Ok(())
}

fn format_gtp_vertex(board_size: usize, column: usize, row: usize) -> String {
    let letter_index = if column >= 8 { column + 1 } else { column };
    let letter = (b'A' + u8::try_from(letter_index).unwrap_or(0)) as char;
    format!("{letter}{}", board_size.saturating_sub(row))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use sabaki_domain_core::gtp::{GtpError, GtpResponse};
    use sabaki_domain_core::{Color, MoveDto, Vertex};

    use super::{EngineController, EngineControllerError};
    use crate::{EngineRecord, GtpTransport};

    #[derive(Debug)]
    struct FixtureTransport {
        commands: Vec<(String, Vec<String>)>,
        replies: VecDeque<GtpResponse>,
    }

    impl FixtureTransport {
        fn ready() -> Self {
            Self {
                commands: Vec::new(),
                replies: VecDeque::from([
                    ok("Fixture"),
                    ok("1.0"),
                    ok(""),
                    ok(""),
                    ok(""),
                    ok(""),
                    ok(""),
                    ok(""),
                    ok("D4"),
                ]),
            }
        }
    }

    impl GtpTransport for FixtureTransport {
        fn send(&mut self, name: &str, arguments: Vec<String>) -> Result<GtpResponse, GtpError> {
            self.commands.push((name.to_owned(), arguments));
            Ok(self.replies.pop_front().unwrap_or_else(|| ok("")))
        }

        fn stop(&mut self) -> Result<(), std::io::Error> {
            Ok(())
        }
    }

    fn ok(content: &str) -> GtpResponse {
        GtpResponse {
            identifier: None,
            success: true,
            content: content.to_owned(),
        }
    }

    #[test]
    fn controller_hides_handshake_replay_and_role_map() {
        let mut controller = EngineController::<u8, FixtureTransport>::default();
        let moves = [MoveDto {
            color: Color::Black,
            vertex: Some(Vertex { column: 3, row: 3 }),
        }];
        controller
            .attach(
                1,
                FixtureTransport::ready(),
                &EngineRecord::new("Fixture", "fixture", ""),
                19,
                &moves,
            )
            .expect("controller attaches and replays");
        assert!(controller.is_attached(1));
        let response = controller
            .request_move(1, Color::White)
            .expect("generates a move");
        assert_eq!(response.content, "D4");
        assert!(controller.detach(1));
        assert!(!controller.is_attached(1));
    }

    #[test]
    fn controller_rejects_duplicate_and_detached_roles() {
        let mut controller = EngineController::<u8, FixtureTransport>::default();
        let record = EngineRecord::new("Fixture", "fixture", "");
        controller
            .attach(1, FixtureTransport::ready(), &record, 19, &[])
            .expect("first attach works");
        assert!(matches!(
            controller.attach(1, FixtureTransport::ready(), &record, 19, &[]),
            Err(EngineControllerError::AlreadyAttached)
        ));
        assert!(matches!(
            controller.request_move(2, Color::Black),
            Err(EngineControllerError::Detached)
        ));
    }
}
