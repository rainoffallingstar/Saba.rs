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
    pub move_annotation: Option<NodeAnnotation>,
    pub position_annotation: Option<NodeAnnotation>,
    pub hotspot: bool,
    pub can_edit_variation: bool,
}

/// One of the upstream CommentBox annotation choices. Move and position
/// choices are mutually exclusive within their respective groups; hotspot is
/// independent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeAnnotation {
    BadMove,
    DoubtfulMove,
    InterestingMove,
    GoodMove,
    UnclearPosition,
    GoodForWhite,
    EvenPosition,
    GoodForBlack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnnotationGroup {
    Move,
    Position,
}

impl NodeAnnotation {
    pub const MOVE: [Self; 4] = [
        Self::BadMove,
        Self::DoubtfulMove,
        Self::InterestingMove,
        Self::GoodMove,
    ];
    pub const POSITION: [Self; 4] = [
        Self::UnclearPosition,
        Self::GoodForWhite,
        Self::EvenPosition,
        Self::GoodForBlack,
    ];

    pub const fn property(self) -> &'static str {
        match self {
            Self::BadMove => "BM",
            Self::DoubtfulMove => "DO",
            Self::InterestingMove => "IT",
            Self::GoodMove => "TE",
            Self::UnclearPosition => "UC",
            Self::GoodForWhite => "GW",
            Self::EvenPosition => "DM",
            Self::GoodForBlack => "GB",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::BadMove => "bad",
            Self::DoubtfulMove => "doubtful",
            Self::InterestingMove => "interesting",
            Self::GoodMove => "good",
            Self::UnclearPosition => "unclear",
            Self::GoodForWhite => "white better",
            Self::EvenPosition => "even",
            Self::GoodForBlack => "black better",
        }
    }

    pub const fn group(self) -> AnnotationGroup {
        match self {
            Self::BadMove | Self::DoubtfulMove | Self::InterestingMove | Self::GoodMove => {
                AnnotationGroup::Move
            }
            Self::UnclearPosition
            | Self::GoodForWhite
            | Self::EvenPosition
            | Self::GoodForBlack => AnnotationGroup::Position,
        }
    }

    const fn values(self) -> &'static [&'static str] {
        match self {
            Self::DoubtfulMove | Self::InterestingMove => &[""],
            _ => &["1"],
        }
    }
}

impl AnnotationGroup {
    const fn properties(self) -> &'static [&'static str] {
        match self {
            Self::Move => &["BM", "DO", "IT", "TE"],
            Self::Position => &["UC", "GW", "DM", "GB"],
        }
    }
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
            move_annotation: None,
            position_annotation: None,
            hotspot: false,
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

    let move_annotation = NodeAnnotation::MOVE
        .into_iter()
        .find(|annotation| node.properties.contains_key(annotation.property()));
    let position_annotation = NodeAnnotation::POSITION
        .into_iter()
        .find(|annotation| node.properties.contains_key(annotation.property()));

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
        move_annotation,
        position_annotation,
        hotspot: node.properties.contains_key("HO"),
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

/// Builds a sequence of transactions that makes one annotation active and
/// clears only its peer group first. The caller applies these in order, so the
/// document never retains conflicting upstream CommentBox markers.
pub fn create_annotation_transactions(
    node_id: &NodeId,
    annotation: Option<NodeAnnotation>,
    group: AnnotationGroup,
) -> Vec<GameTransaction> {
    let mut transactions = group
        .properties()
        .iter()
        .map(|property| create_property_transaction(node_id, property, Vec::new()))
        .collect::<Vec<_>>();
    if let Some(annotation) = annotation {
        assert_eq!(annotation.group(), group, "annotation must match its group");
        transactions.push(create_property_transaction(
            node_id,
            annotation.property(),
            annotation
                .values()
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        ));
    }
    transactions
}

/// Builds the independent CommentBox hotspot transaction.
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
        AnnotationGroup, NodeAnnotation, VariationAction, create_annotation_transactions,
        create_comment_transaction, create_hotspot_transaction, create_variation_transaction,
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
    fn annotations_clear_only_their_peer_group_then_set_the_selected_value() {
        let transactions = create_annotation_transactions(
            &"node-1".to_owned(),
            Some(NodeAnnotation::InterestingMove),
            AnnotationGroup::Move,
        );
        assert_eq!(transactions.len(), 5);
        assert!(transactions[..4].iter().all(|transaction| {
            transaction.transaction_type == GameTransactionType::RemoveNodeProperty
        }));
        assert_eq!(transactions[4].property.as_deref(), Some("IT"));
        assert_eq!(transactions[4].values, vec!["".to_owned()]);

        let clear =
            create_annotation_transactions(&"node-1".to_owned(), None, AnnotationGroup::Position);
        assert_eq!(clear.len(), 4);
        assert!(clear.iter().all(|transaction| {
            transaction.transaction_type == GameTransactionType::RemoveNodeProperty
        }));
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
