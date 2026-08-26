//! Metadata and transactions for the right-sidebar comment editor.
//!
//! Only the comment box survives in the current UI: the node title, the SGF
//! comment, and the comment `C` property transactions. Annotation buttons,
//! variation promote/remove, and the property table were removed with their
//! UI; hotspot editing lives in the game-graph context menu and reuses
//! `create_hotspot_transaction`.

use ryusei_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, GameSnapshot, GameTransaction, GameTransactionType, NodeId,
};

/// The human-readable heading for the current node.
fn node_title(properties: &ryusei_domain_core::Properties) -> String {
    if let Some(name) = properties.get("N").and_then(|values| values.first())
        && !name.trim().is_empty()
    {
        return name.clone();
    }
    if properties.contains_key("B") {
        return "Black move".to_owned();
    }
    if properties.contains_key("W") {
        return "White move".to_owned();
    }
    "Game information".to_owned()
}

/// UI-relevant metadata extracted from a snapshot's current node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeInspectorMetadata {
    pub node_id: NodeId,
    pub title: String,
    pub comment: String,
}

/// Extracts the comment editor metadata for the current node.
pub fn current_node_metadata(snapshot: &GameSnapshot) -> NodeInspectorMetadata {
    let node = snapshot
        .nodes
        .iter()
        .find(|node| node.id == snapshot.current_node_id);
    let Some(node) = node else {
        return NodeInspectorMetadata {
            node_id: snapshot.current_node_id.clone(),
            title: "Unavailable node".to_owned(),
            comment: String::new(),
        };
    };

    NodeInspectorMetadata {
        node_id: node.id.clone(),
        title: node_title(&node.properties),
        comment: node
            .properties
            .get("C")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default(),
    }
}

/// Builds a `SetNodeProperty` / `RemoveNodeProperty` transaction for the
/// comment. An empty comment removes the `C` property.
pub fn create_comment_transaction(node_id: &NodeId, comment: &str) -> GameTransaction {
    let values = if comment.trim().is_empty() {
        Vec::new()
    } else {
        vec![comment.to_owned()]
    };
    create_property_transaction(node_id, "C", values)
}

/// Builds a property transaction. Empty values remove the property.
pub fn create_property_transaction(
    node_id: &NodeId,
    property: &str,
    values: Vec<String>,
) -> GameTransaction {
    GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: if values.is_empty() {
            GameTransactionType::RemoveNodeProperty
        } else {
            GameTransactionType::SetNodeProperty
        },
        color: None,
        vertex: None,
        node_id: Some(node_id.clone()),
        property: Some(property.to_owned()),
        values,
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

/// Builds the independent CommentBox hotspot transaction (used by the
/// game-graph context menu).
pub fn create_hotspot_transaction(node_id: &NodeId, enabled: bool) -> GameTransaction {
    create_property_transaction(
        node_id,
        "HO",
        if enabled {
            vec!["1".to_owned()]
        } else {
            Default::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{create_comment_transaction, create_hotspot_transaction, current_node_metadata};
    use ryusei_domain_core::{GameDocument, GameTransactionType};

    fn snapshot_of(sgf: &str) -> ryusei_domain_core::GameSnapshot {
        GameDocument::from_sgf(sgf).unwrap().snapshot()
    }

    #[test]
    fn extracts_title_and_comment_from_the_current_node() {
        let snapshot = snapshot_of("(;SZ[19];B[pd]N[Opening]C[first move]TE[1])");
        let metadata = current_node_metadata(&snapshot);

        assert_eq!(metadata.title, "Opening");
        assert_eq!(metadata.comment, "first move");
    }

    #[test]
    fn falls_back_to_move_and_game_titles() {
        assert_eq!(
            current_node_metadata(&snapshot_of("(;SZ[19];B[pd])")).title,
            "Black move"
        );
        assert_eq!(
            current_node_metadata(&snapshot_of("(;SZ[19])")).title,
            "Game information"
        );
    }

    #[test]
    fn empty_comment_builds_a_removal_transaction() {
        let transaction = create_comment_transaction(&"node-1".to_owned(), "  ");
        assert_eq!(
            transaction.transaction_type,
            GameTransactionType::RemoveNodeProperty
        );
        assert_eq!(transaction.property.as_deref(), Some("C"));
        assert!(transaction.values.is_empty());
    }

    #[test]
    fn non_empty_comment_builds_a_set_transaction() {
        let transaction = create_comment_transaction(&"node-1".to_owned(), "good game");
        assert_eq!(
            transaction.transaction_type,
            GameTransactionType::SetNodeProperty
        );
        assert_eq!(transaction.values, vec!["good game".to_owned()]);
    }

    #[test]
    fn hotspot_is_independent_and_uses_the_standard_value() {
        let set = create_hotspot_transaction(&"node-1".to_owned(), true);
        assert_eq!(set.property.as_deref(), Some("HO"));
        assert_eq!(set.values, vec!["1".to_owned()]);
        assert_eq!(
            create_hotspot_transaction(&"node-1".to_owned(), false).transaction_type,
            GameTransactionType::RemoveNodeProperty
        );
    }

    #[test]
    fn unavailable_node_renders_a_placeholder() {
        let snapshot = snapshot_of("(;SZ[19])");
        let mut snapshot = snapshot;
        snapshot.current_node_id = "missing".to_owned();
        let metadata = current_node_metadata(&snapshot);
        assert_eq!(metadata.title, "Unavailable node");
        assert!(metadata.comment.is_empty());
    }
}
