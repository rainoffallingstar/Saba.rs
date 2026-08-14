use sabaki_domain_core::{
    CURRENT_TRANSACTION_SCHEMA_VERSION, GameSnapshot, GameTransaction, GameTransactionType, NodeId,
};

/// The display title for known SGF properties, mirroring the reference UI.
fn property_display_name(property: &str) -> &'static str {
    match property {
        "B" => "Black move",
        "W" => "White move",
        "C" => "Comment",
        "N" => "Node name",
        "HO" => "Hotspot",
        "BM" => "Bad move",
        "DO" => "Doubtful move",
        "IT" => "Interesting move",
        "TE" => "Good move",
        "UC" => "Unclear position",
        "GW" => "Good for White",
        "GB" => "Good for Black",
        "DM" => "Even position",
        _ => "Property",
    }
}

/// The human-readable heading for the current node.
fn node_title(properties: &sabaki_domain_core::Properties) -> String {
    if let Some(name) = properties.get("N").and_then(|values| values.first()) {
        if !name.trim().is_empty() {
            return name.clone();
        }
    }
    if properties.contains_key("B") {
        return "Black move".to_owned();
    }
    if properties.contains_key("W") {
        return "White move".to_owned();
    }
    "Game information".to_owned()
}

/// A rendered property row, excluding the comment which is edited separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodePropertyRow {
    pub property: String,
    pub name: String,
    pub value: String,
}

/// UI-relevant metadata extracted from a snapshot's current node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeInspectorMetadata {
    pub node_id: NodeId,
    pub title: String,
    pub comment: String,
    pub properties: Vec<NodePropertyRow>,
    pub can_edit_variation: bool,
}

/// Extracts metadata for the current node from a snapshot. Pure function so it
/// can be unit-tested without a view.
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
            properties: Vec::new(),
            can_edit_variation: false,
        };
    };

    let mut properties: Vec<NodePropertyRow> = node
        .properties
        .iter()
        .filter(|(property, _)| property.as_str() != "C")
        .map(|(property, values)| NodePropertyRow {
            property: property.clone(),
            name: property_display_name(property).to_owned(),
            value: values.join(", "),
        })
        .collect();
    properties.sort_by(|left, right| left.name.cmp(&right.name));

    NodeInspectorMetadata {
        node_id: node.id.clone(),
        title: node_title(&node.properties),
        comment: node
            .properties
            .get("C")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default(),
        properties,
        can_edit_variation: node.parent_id.is_some(),
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

/// Builds a `PromoteVariation` / `RemoveVariation` transaction for a node.
pub fn create_variation_transaction(node_id: &NodeId, action: VariationAction) -> GameTransaction {
    GameTransaction {
        schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
        transaction_type: match action {
            VariationAction::Promote => GameTransactionType::PromoteVariation,
            VariationAction::Remove => GameTransactionType::RemoveVariation,
        },
        color: None,
        vertex: None,
        node_id: Some(node_id.clone()),
        property: None,
        values: Vec::new(),
        marker: None,
        nodes: Vec::new(),
        score_override: None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariationAction {
    Promote,
    Remove,
}

#[cfg(test)]
mod tests {
    use super::{
        VariationAction, create_comment_transaction, create_variation_transaction,
        current_node_metadata,
    };
    use sabaki_domain_core::{GameDocument, GameTransactionType};

    fn snapshot_of(sgf: &str) -> sabaki_domain_core::GameSnapshot {
        GameDocument::from_sgf(sgf).unwrap().snapshot()
    }

    #[test]
    fn extracts_title_comment_and_properties_from_the_current_node() {
        let snapshot = snapshot_of("(;SZ[19];B[pd]N[Opening]C[first move]TE[1])");
        let metadata = current_node_metadata(&snapshot);

        assert_eq!(metadata.title, "Opening");
        assert_eq!(metadata.comment, "first move");
        assert!(metadata.can_edit_variation);
        assert!(
            metadata
                .properties
                .iter()
                .any(|row| row.property == "TE" && row.name == "Good move" && row.value == "1")
        );
        assert!(
            !metadata.properties.iter().any(|row| row.property == "C"),
            "the comment is excluded from the property list"
        );
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
    fn variation_actions_map_to_their_transactions() {
        let promote = create_variation_transaction(&"node-2".to_owned(), VariationAction::Promote);
        assert_eq!(
            promote.transaction_type,
            GameTransactionType::PromoteVariation
        );
        assert_eq!(promote.node_id.as_deref(), Some("node-2"));

        let remove = create_variation_transaction(&"node-2".to_owned(), VariationAction::Remove);
        assert_eq!(
            remove.transaction_type,
            GameTransactionType::RemoveVariation
        );
    }

    #[test]
    fn unavailable_node_renders_a_placeholder() {
        let snapshot = snapshot_of("(;SZ[19])");
        let mut snapshot = snapshot;
        snapshot.current_node_id = "missing".to_owned();
        let metadata = current_node_metadata(&snapshot);
        assert_eq!(metadata.title, "Unavailable node");
        assert!(!metadata.can_edit_variation);
    }
}
