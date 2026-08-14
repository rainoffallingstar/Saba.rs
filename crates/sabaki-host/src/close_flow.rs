#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseRequestAction {
    Allow,
    Prevent,
    ConfirmDiscard,
}

pub fn decide_close_request(
    is_document_dirty: bool,
    is_confirmation_pending: bool,
) -> CloseRequestAction {
    if !is_document_dirty {
        CloseRequestAction::Allow
    } else if is_confirmation_pending {
        CloseRequestAction::Prevent
    } else {
        CloseRequestAction::ConfirmDiscard
    }
}

#[cfg(test)]
mod tests {
    use super::{CloseRequestAction, decide_close_request};

    #[test]
    fn permits_clean_documents_to_close_without_a_prompt() {
        assert_eq!(
            decide_close_request(false, false),
            CloseRequestAction::Allow
        );
    }

    #[test]
    fn prompts_once_for_dirty_documents_and_blocks_duplicate_requests() {
        assert_eq!(
            decide_close_request(true, false),
            CloseRequestAction::ConfirmDiscard
        );
        assert_eq!(
            decide_close_request(true, true),
            CloseRequestAction::Prevent
        );
    }
}
