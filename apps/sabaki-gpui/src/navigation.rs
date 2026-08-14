use sabaki_domain_core::{GameSnapshot, NodeId};

/// The four navigation commands of the shell navigation bar. The target node
/// is derived from the snapshot so the navigation logic stays a pure, testable
/// function instead of living in the view layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationDirection {
    First,
    Previous,
    Next,
    Last,
}

/// Computes the node the given navigation command should move to, or `None`
/// when the command has no legal target (for example "previous" on the root).
pub fn navigation_target(
    snapshot: &GameSnapshot,
    direction: NavigationDirection,
) -> Option<NodeId> {
    let current = snapshot.current_node_id.clone();
    match direction {
        NavigationDirection::First => Some(snapshot.root_node_id.clone()),
        NavigationDirection::Previous => parent_of(&current, snapshot),
        NavigationDirection::Next => next_of(&current, snapshot),
        NavigationDirection::Last => last_in_branch(&current, snapshot),
    }
}

fn parent_of(node_id: &str, snapshot: &GameSnapshot) -> Option<NodeId> {
    snapshot
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .and_then(|node| node.parent_id.clone())
}

fn next_of(node_id: &str, snapshot: &GameSnapshot) -> Option<NodeId> {
    if let Some(preferred_child) = snapshot.preferred_child_by_node.get(node_id) {
        return Some(preferred_child.clone());
    }
    snapshot
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .and_then(|node| node.child_ids.first().cloned())
}

fn last_in_branch(node_id: &str, snapshot: &GameSnapshot) -> Option<NodeId> {
    let mut cursor = node_id.to_owned();
    while let Some(next) = next_of(&cursor, snapshot) {
        cursor = next;
    }
    Some(cursor)
}

/// Builds the `Navigate` transaction target list for the shell UI, mirroring
/// the enabled/disabled state of the four navigation buttons.
pub fn navigation_availability(snapshot: &GameSnapshot) -> NavigationAvailability {
    let current = snapshot.current_node_id.clone();
    NavigationAvailability {
        can_go_first: snapshot.current_node_id != snapshot.root_node_id,
        can_go_previous: parent_of(&current, snapshot).is_some(),
        can_go_next: next_of(&current, snapshot).is_some(),
        can_go_last: next_of(&current, snapshot).is_some(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationAvailability {
    pub can_go_first: bool,
    pub can_go_previous: bool,
    pub can_go_next: bool,
    pub can_go_last: bool,
}

/// A compact position label such as "3/5" for the current node in its branch.
pub fn position_label(snapshot: &GameSnapshot) -> String {
    let mut ancestors = 0;
    let mut cursor = snapshot.current_node_id.clone();
    while let Some(parent) = parent_of(&cursor, snapshot) {
        ancestors += 1;
        cursor = parent;
    }
    let mut descendants = 0;
    let mut cursor = snapshot.current_node_id.clone();
    while let Some(next) = next_of(&cursor, snapshot) {
        descendants += 1;
        cursor = next;
    }
    format!("{}/{}", ancestors + 1, ancestors + descendants + 1)
}

#[cfg(test)]
mod tests {
    use super::{NavigationDirection, navigation_availability, navigation_target, position_label};
    use sabaki_domain_core::{GameDocument, GameSnapshot};

    fn snapshot_of(sgf: &str) -> GameSnapshot {
        GameDocument::from_sgf(sgf).unwrap().snapshot()
    }

    fn navigate(snapshot: &GameSnapshot, direction: NavigationDirection) -> Option<String> {
        navigation_target(snapshot, direction)
    }

    #[test]
    fn linear_game_has_first_previous_next_last_targets() {
        let snapshot = snapshot_of("(;SZ[5];B[aa];W[bb];B[cc])");
        assert_eq!(snapshot.nodes.len(), 4);

        assert_eq!(
            navigate(&snapshot, NavigationDirection::First),
            Some("root".to_owned()),
            "first always points at the root node id"
        );
        assert_eq!(
            navigate(&snapshot, NavigationDirection::Previous),
            Some("node-2".to_owned()),
            "previous walks one node back"
        );
        assert_eq!(
            navigate(&snapshot, NavigationDirection::Last),
            Some("node-3".to_owned()),
            "last walks to the end of the branch"
        );
        assert_eq!(
            navigate(&snapshot, NavigationDirection::Next),
            None,
            "the last node has no next target"
        );
    }

    #[test]
    fn first_previous_are_disabled_at_the_root() {
        let snapshot = snapshot_of("(;SZ[5])");
        assert_eq!(
            navigate(&snapshot, NavigationDirection::First),
            Some("root".to_owned()),
            "first on the root is a no-op but still returns the root"
        );
        assert_eq!(navigate(&snapshot, NavigationDirection::Previous), None);
        assert_eq!(navigate(&snapshot, NavigationDirection::Next), None);

        let availability = navigation_availability(&snapshot);
        assert!(!availability.can_go_first);
        assert!(!availability.can_go_previous);
        assert!(!availability.can_go_next);
        assert!(!availability.can_go_last);
    }

    #[test]
    fn branching_game_next_follows_the_preferred_child() {
        let mut game = GameDocument::from_sgf("(;SZ[5];B[aa](;W[bb])(;W[cc]))").unwrap();
        game.apply_transaction(sabaki_domain_core::GameTransaction {
            schema_version: sabaki_domain_core::CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type: sabaki_domain_core::GameTransactionType::Navigate,
            color: None,
            vertex: None,
            node_id: Some("node-1".to_owned()),
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override: None,
        })
        .unwrap();
        let snapshot = game.snapshot();

        assert_eq!(snapshot.current_node_id, "node-1");
        assert_eq!(
            navigate(&snapshot, NavigationDirection::Next),
            snapshot
                .preferred_child_by_node
                .get(&snapshot.current_node_id)
                .cloned(),
            "next follows the preferred child when available"
        );
        assert_eq!(
            navigate(&snapshot, NavigationDirection::Next),
            Some("node-2".to_owned())
        );

        let availability = navigation_availability(&snapshot);
        assert!(availability.can_go_next);
        assert!(availability.can_go_last);
    }

    #[test]
    fn position_label_counts_ancestors_and_descendants_in_branch() {
        let snapshot = snapshot_of("(;SZ[5];B[aa];W[bb];B[cc])");
        assert_eq!(position_label(&snapshot), "4/4");

        let root_snapshot = snapshot_of("(;SZ[5])");
        assert_eq!(position_label(&root_snapshot), "1/1");
    }
}
