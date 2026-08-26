//! Analysis-run lifecycle Module.
//!
//! GPUI owns task scheduling, while this Module owns the thread-safe run state
//! shared by the shell and its workers: generation invalidation, cooperative
//! stop, session disposal, replay requests, and position binding for persisted
//! results.

use std::sync::{Arc, Mutex};

use ryusei_domain_core::{Color, NodeId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisRunOutcome {
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct AnalysisRunTicket {
    shared: Arc<Mutex<AnalysisRunState>>,
    generation: usize,
}

impl AnalysisRunTicket {
    pub fn generation(&self) -> usize {
        self.generation
    }

    /// The document node captured when this run began.
    pub fn node_id(&self) -> NodeId {
        self.shared
            .lock()
            .expect("analysis run state is not poisoned")
            .node_id
            .clone()
            .expect("analysis run ticket always has a bound node")
    }

    /// A worker stops when a newer run invalidated it or a caller requested a
    /// cooperative stop. This is safe to call frequently from a stream loop.
    pub fn should_stop(&self) -> bool {
        let state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        state.generation != self.generation || state.stop_requested
    }

    pub fn is_current(&self) -> bool {
        self.shared
            .lock()
            .expect("analysis run state is not poisoned")
            .generation
            == self.generation
    }

    /// A worker must dispose its leased engine session when a newer run has
    /// invalidated this ticket. Keeping this query on the ticket lets the
    /// worker make the ownership decision without borrowing the UI controller.
    pub fn should_dispose(&self) -> bool {
        !self.is_current()
            || self
                .shared
                .lock()
                .expect("analysis run state is not poisoned")
                .dispose_session
    }
}

#[derive(Debug, Default)]
pub struct AnalysisRunController {
    shared: Arc<Mutex<AnalysisRunState>>,
}

#[derive(Debug, Default)]
struct AnalysisRunState {
    generation: usize,
    stop_requested: bool,
    dispose_session: bool,
    replay_required: bool,
    node_id: Option<NodeId>,
    player: Option<Color>,
}

impl AnalysisRunController {
    /// Begins a new run and invalidates every prior ticket. The ticket is the
    /// only value an async worker needs to observe cancellation safely.
    pub fn begin(&self, node_id: NodeId, player: Color) -> AnalysisRunTicket {
        let mut state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        state.generation += 1;
        state.stop_requested = false;
        state.dispose_session = false;
        state.replay_required = false;
        state.node_id = Some(node_id);
        state.player = Some(player);
        AnalysisRunTicket {
            shared: self.shared.clone(),
            generation: state.generation,
        }
    }

    /// Requests a cooperative stop without invalidating the ticket, allowing a
    /// worker to flush its final batch and return a leased session.
    pub fn request_stop(&self) {
        self.shared
            .lock()
            .expect("analysis run state is not poisoned")
            .stop_requested = true;
    }

    /// Requests a stop and position replay when a local move arrives while a
    /// streaming session is leased.
    pub fn request_replay_and_stop(&self) {
        let mut state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        state.replay_required = true;
        state.stop_requested = true;
    }

    /// Invalidates the current worker and marks any leased session for disposal.
    pub fn cancel_and_dispose(&self) {
        let mut state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        state.generation += 1;
        state.stop_requested = true;
        state.dispose_session = true;
        state.replay_required = false;
    }

    /// Returns whether this current ticket must replay before its session is
    /// returned to the controller.
    pub fn replay_required(&self, ticket: &AnalysisRunTicket) -> bool {
        let state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        state.generation == ticket.generation && state.replay_required
    }

    pub fn clear_replay(&self, ticket: &AnalysisRunTicket) {
        let mut state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        if state.generation == ticket.generation {
            state.replay_required = false;
        }
    }

    /// A leased session must be discarded after cancellation or explicit
    /// disconnect, even if its task wakes up later.
    pub fn should_dispose(&self, ticket: &AnalysisRunTicket) -> bool {
        let state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        state.generation != ticket.generation || state.dispose_session
    }

    /// Clears current run flags once its matching worker is done. Stale workers
    /// cannot reset the state of a newer run.
    pub fn finish(&self, ticket: &AnalysisRunTicket) -> bool {
        let mut state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        if state.generation != ticket.generation {
            return false;
        }
        state.stop_requested = false;
        state.dispose_session = false;
        true
    }

    /// Returns the player binding only when an analysis result still belongs to
    /// the given document node; callers use it before persisting SGF metadata.
    pub fn player_for_node(&self, node_id: &NodeId) -> Option<Color> {
        let state = self
            .shared
            .lock()
            .expect("analysis run state is not poisoned");
        (state.node_id.as_ref() == Some(node_id))
            .then_some(state.player)
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use ryusei_domain_core::Color;

    use super::AnalysisRunController;

    #[test]
    fn tickets_stop_on_request_and_invalidation() {
        let controller = AnalysisRunController::default();
        let first = controller.begin("one".to_owned(), Color::Black);
        assert!(!first.should_stop());
        assert_eq!(first.node_id(), "one");
        controller.request_stop();
        assert!(first.should_stop());

        let second = controller.begin("two".to_owned(), Color::White);
        assert!(first.should_stop());
        assert!(!second.should_stop());
        assert_eq!(
            controller.player_for_node(&"two".to_owned()),
            Some(Color::White)
        );
        assert_eq!(controller.player_for_node(&"one".to_owned()), None);
    }

    #[test]
    fn replay_and_disposal_are_scoped_to_the_current_ticket() {
        let controller = AnalysisRunController::default();
        let ticket = controller.begin("one".to_owned(), Color::Black);
        controller.request_replay_and_stop();
        assert!(controller.replay_required(&ticket));
        controller.clear_replay(&ticket);
        assert!(!controller.replay_required(&ticket));
        assert!(controller.finish(&ticket));

        controller.cancel_and_dispose();
        assert!(ticket.should_dispose());
        assert!(controller.should_dispose(&ticket));
        assert!(!controller.finish(&ticket));
    }
}
