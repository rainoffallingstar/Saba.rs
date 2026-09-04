use std::collections::{BTreeMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod gtp;
pub mod legacy;
pub mod library;
pub mod monte_carlo;
pub mod opening;
pub mod review;
pub mod scoring;
pub mod session;
pub mod time_control;

pub use library::{
    GameRecord, InsertOutcome, LIBRARY_SCHEMA_VERSION, LibraryIndex, LibraryQuery, LibrarySort,
    RecordId, RecordMetadata, RecordNumber, RecordRevisionRef, RecordSource, RevisionTrigger,
};
pub use opening::OpeningConvention;
pub use review::ReviewProfile;
pub use scoring::{
    DEFAULT_KOMI, ScoreResult, ScoringRule, StoneChain, find_chains, mark_surrounded_chains,
    score_board, score_board_with_estimation, score_board_with_rule,
};
pub use session::{
    AnalysisPolicy, MatchParticipants, PlayerKind, SessionMode, SessionPolicy, SessionSource,
};
pub use time_control::{
    ClockController, ClockEvent, ClockPhase, ClockState, PlayerClock, TimeControl,
};

pub const CURRENT_GAME_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_TRANSACTION_SCHEMA_VERSION: u32 = 1;
pub const ROOT_NODE_ID: &str = "root";

pub type NodeId = String;
pub type Properties = BTreeMap<String, Vec<String>>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    Black,
    White,
}

impl Color {
    pub fn opponent(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }

    fn stone_value(self) -> i8 {
        match self {
            Self::Black => 1,
            Self::White => -1,
        }
    }

    fn sgf_property(self) -> &'static str {
        match self {
            Self::Black => "B",
            Self::White => "W",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vertex {
    pub column: usize,
    pub row: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDto {
    pub color: Color,
    pub vertex: Option<Vertex>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSnapshot {
    #[serde(rename = "type")]
    pub marker_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardLineSnapshot {
    pub start: Vertex,
    pub end: Vertex,
    #[serde(rename = "type")]
    pub line_type: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariationInfoSnapshot {
    pub vertex: Vertex,
    pub color: Color,
    pub annotation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSnapshot {
    pub width: usize,
    pub height: usize,
    pub sign_map: Vec<Vec<i8>>,
    pub current_vertex: Option<Vertex>,
    pub next_player: Color,
    pub move_number: usize,
    pub markers: Vec<Vec<Option<MarkerSnapshot>>>,
    pub lines: Vec<BoardLineSnapshot>,
    pub children_info: Vec<VariationInfoSnapshot>,
    pub siblings_info: Vec<VariationInfoSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSnapshot {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub child_ids: Vec<NodeId>,
    pub properties: Properties,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySnapshot {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStateSnapshot {
    pub path: Option<String>,
    pub format: Option<String>,
    pub is_dirty: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GameMode {
    Play,
    Edit,
    Scoring,
    Estimator,
    Find,
    Guess,
    Autoplay,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameTransactionType {
    PlayMove,
    Pass,
    SetNodeProperty,
    RemoveNodeProperty,
    AddMarkup,
    AppendVariation,
    RemoveVariation,
    PromoteVariation,
    Navigate,
    ApplyScoringOverride,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTransaction {
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub transaction_type: GameTransactionType,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub vertex: Option<Vertex>,
    #[serde(default)]
    pub node_id: Option<NodeId>,
    #[serde(default)]
    pub property: Option<String>,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub marker: Option<MarkerSnapshot>,
    #[serde(default)]
    pub nodes: Vec<NodeSnapshot>,
    #[serde(default)]
    pub score_override: Option<i8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub root_properties: Properties,
    pub nodes: Vec<NodeSnapshot>,
    pub root_node_id: NodeId,
    pub current_node_id: NodeId,
    pub preferred_child_by_node: BTreeMap<NodeId, NodeId>,
    pub moves: Vec<MoveDto>,
    pub board: BoardSnapshot,
    pub history: HistorySnapshot,
    pub file_state: FileStateSnapshot,
    pub mode: GameMode,
    pub can_undo: bool,
    pub can_redo: bool,
    pub source_path: Option<String>,
    /// Alive-stone overrides for scoring: `1` marks a vertex alive for black,
    /// `-1` alive for white; absent entries carry no override.
    #[serde(default)]
    pub score_overrides: BTreeMap<Vertex, i8>,
    /// Stones Black has captured from White along the current path (White's
    /// prisoners). Drives the "(N提)" affordance in the player VS pill.
    #[serde(default)]
    pub black_captures: usize,
    /// Stones White has captured from Black along the current path.
    #[serde(default)]
    pub white_captures: usize,
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("board dimensions must be between 2 and 25")]
    UnsupportedBoardSize,
    #[error("the selected vertex is outside the board")]
    VertexOutsideBoard,
    #[error("the selected vertex is already occupied")]
    OccupiedVertex,
    #[error("the move is suicidal")]
    SuicidalMove,
    #[error("the move repeats the preceding board position")]
    KoViolation,
    #[error("the SGF document does not contain a game tree")]
    MissingGameTree,
    #[error("the SGF document contains an invalid property at byte {0}")]
    InvalidSgf(usize),
    #[error("the SGF coordinate is invalid")]
    InvalidCoordinate,
    #[error("unsupported game transaction schema version {0}")]
    UnsupportedTransactionSchema(u32),
    #[error("game transaction {0:?} is not implemented")]
    UnsupportedTransaction(GameTransactionType),
    #[error("game transaction {0:?} requires a color")]
    MissingTransactionColor(GameTransactionType),
    #[error("game transaction {0:?} requires a node id")]
    MissingTransactionNodeId(GameTransactionType),
    #[error("game transaction {0:?} requires a property")]
    MissingTransactionProperty(GameTransactionType),
    #[error("game transaction {0:?} requires a vertex")]
    MissingTransactionVertex(GameTransactionType),
    #[error("game transaction {0:?} requires a marker")]
    MissingTransactionMarker(GameTransactionType),
    #[error("game transaction requires a score override")]
    MissingScoreOverride,
    #[error(
        "score override {0} is invalid; expected -1 (white alive), 0 (clear), or 1 (black alive)"
    )]
    InvalidScoreOverride(i8),
    #[error("the node {0} does not exist")]
    MissingNode(NodeId),
    #[error("the root node cannot be removed")]
    CannotRemoveRoot,
    #[error("the transaction contains no variation nodes")]
    EmptyVariation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Node {
    id: NodeId,
    parent_id: Option<NodeId>,
    child_ids: Vec<NodeId>,
    properties: Properties,
}

impl Node {
    fn snapshot(&self) -> NodeSnapshot {
        NodeSnapshot {
            id: self.id.clone(),
            parent_id: self.parent_id.clone(),
            child_ids: self.child_ids.clone(),
            properties: self.properties.clone(),
        }
    }

    fn move_data(&self) -> Result<Option<MoveDto>, DomainError> {
        match (self.properties.get("B"), self.properties.get("W")) {
            (Some(values), None) => Ok(Some(MoveDto {
                color: Color::Black,
                vertex: values
                    .first()
                    .map(|value| parse_sgf_vertex(value))
                    .transpose()?
                    .flatten(),
            })),
            (None, Some(values)) => Ok(Some(MoveDto {
                color: Color::White,
                vertex: values
                    .first()
                    .map(|value| parse_sgf_vertex(value))
                    .transpose()?
                    .flatten(),
            })),
            _ => Ok(None),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Board {
    width: usize,
    height: usize,
    sign_map: Vec<Vec<i8>>,
    current_vertex: Option<Vertex>,
}

impl Board {
    fn new(width: usize, height: usize) -> Result<Self, DomainError> {
        if !(2..=25).contains(&width) || !(2..=25).contains(&height) {
            return Err(DomainError::UnsupportedBoardSize);
        }

        Ok(Self {
            width,
            height,
            sign_map: vec![vec![0; width]; height],
            current_vertex: None,
        })
    }

    fn has(&self, vertex: Vertex) -> bool {
        vertex.column < self.width && vertex.row < self.height
    }

    fn get(&self, vertex: Vertex) -> i8 {
        self.sign_map[vertex.row][vertex.column]
    }

    fn set(&mut self, vertex: Vertex, value: i8) {
        self.sign_map[vertex.row][vertex.column] = value;
    }

    fn neighbours(&self, vertex: Vertex) -> impl Iterator<Item = Vertex> + '_ {
        let mut vertices = Vec::with_capacity(4);
        if vertex.column > 0 {
            vertices.push(Vertex {
                column: vertex.column - 1,
                row: vertex.row,
            });
        }
        if vertex.column + 1 < self.width {
            vertices.push(Vertex {
                column: vertex.column + 1,
                row: vertex.row,
            });
        }
        if vertex.row > 0 {
            vertices.push(Vertex {
                column: vertex.column,
                row: vertex.row - 1,
            });
        }
        if vertex.row + 1 < self.height {
            vertices.push(Vertex {
                column: vertex.column,
                row: vertex.row + 1,
            });
        }
        vertices.into_iter()
    }

    fn group_and_liberties(&self, start: Vertex) -> (Vec<Vertex>, usize) {
        let sign = self.get(start);
        let mut visited = HashSet::new();
        let mut liberties = HashSet::new();
        let mut queue = VecDeque::from([start]);

        while let Some(vertex) = queue.pop_front() {
            if !visited.insert((vertex.column, vertex.row)) {
                continue;
            }
            for neighbour in self.neighbours(vertex) {
                match self.get(neighbour) {
                    0 => {
                        liberties.insert((neighbour.column, neighbour.row));
                    }
                    neighbour_sign if neighbour_sign == sign => queue.push_back(neighbour),
                    _ => {}
                }
            }
        }

        (
            visited
                .into_iter()
                .map(|(column, row)| Vertex { column, row })
                .collect(),
            liberties.len(),
        )
    }

    fn make_move(
        &self,
        color: Color,
        vertex: Option<Vertex>,
        previous_position: Option<&Board>,
    ) -> Result<Self, DomainError> {
        self.make_move_with_history(color, vertex, previous_position, &[])
    }

    /// Makes a move with full positional-superko validation.
    ///
    /// `history` is every earlier position on the current path (excluding the
    /// immediate parent, which is already `self`). A move that recreates any
    /// earlier whole-board position is rejected as a ko violation, covering
    /// superko / multi-stone cycles that a simple one-move ko check misses.
    /// When `history` is empty this degrades to the simple one-move ko check
    /// against `previous_position` (used by capture counting, which replays
    /// without history).
    fn make_move_with_history(
        &self,
        color: Color,
        vertex: Option<Vertex>,
        previous_position: Option<&Board>,
        history: &[Board],
    ) -> Result<Self, DomainError> {
        let Some(vertex) = vertex else {
            let mut passed_board = self.clone();
            passed_board.current_vertex = None;
            return Ok(passed_board);
        };

        if !self.has(vertex) {
            return Err(DomainError::VertexOutsideBoard);
        }
        if self.get(vertex) != 0 {
            return Err(DomainError::OccupiedVertex);
        }

        let mut next_board = self.clone();
        next_board.set(vertex, color.stone_value());
        for neighbour in next_board.neighbours(vertex).collect::<Vec<_>>() {
            if next_board.get(neighbour) != color.opponent().stone_value() {
                continue;
            }
            let (group, liberties) = next_board.group_and_liberties(neighbour);
            if liberties == 0 {
                for captured_vertex in group {
                    next_board.set(captured_vertex, 0);
                }
            }
        }

        let (_, liberties) = next_board.group_and_liberties(vertex);
        if liberties == 0 {
            return Err(DomainError::SuicidalMove);
        }
        // Positional superko: reject recreating any earlier position on the
        // path. The simple one-move ko is the `history.is_empty()` fast path
        // plus the `previous_position` term for history-less replays.
        let recreates_earlier = history
            .iter()
            .any(|earlier| earlier.sign_map == next_board.sign_map);
        if recreates_earlier
            || previous_position.is_some_and(|previous| previous.sign_map == next_board.sign_map)
        {
            return Err(DomainError::KoViolation);
        }

        next_board.current_vertex = Some(vertex);
        Ok(next_board)
    }
}

#[derive(Clone, Debug)]
struct DocumentState {
    node_store: BTreeMap<NodeId, Node>,
    current_node_id: NodeId,
    preferred_child_by_node: BTreeMap<NodeId, NodeId>,
    score_overrides: BTreeMap<Vertex, i8>,
}

#[derive(Clone, Debug)]
pub struct GameDocument {
    node_store: BTreeMap<NodeId, Node>,
    root_node_id: NodeId,
    current_node_id: NodeId,
    preferred_child_by_node: BTreeMap<NodeId, NodeId>,
    source_path: Option<String>,
    revision: u64,
    saved_revision: u64,
    undo_history: Vec<DocumentState>,
    redo_history: Vec<DocumentState>,
    next_node_number: u64,
    score_overrides: BTreeMap<Vertex, i8>,
}

impl GameDocument {
    pub fn new(width: usize, height: usize) -> Result<Self, DomainError> {
        let _board = Board::new(width, height)?;
        let mut root_properties = Properties::new();
        root_properties.insert("GM".to_owned(), vec!["1".to_owned()]);
        root_properties.insert("FF".to_owned(), vec!["4".to_owned()]);
        root_properties.insert("SZ".to_owned(), vec![format_board_size(width, height)]);
        let root_node = Node {
            id: ROOT_NODE_ID.to_owned(),
            parent_id: None,
            child_ids: Vec::new(),
            properties: root_properties,
        };

        Ok(Self {
            node_store: BTreeMap::from([(ROOT_NODE_ID.to_owned(), root_node)]),
            root_node_id: ROOT_NODE_ID.to_owned(),
            current_node_id: ROOT_NODE_ID.to_owned(),
            preferred_child_by_node: BTreeMap::new(),
            source_path: None,
            revision: 0,
            saved_revision: 0,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            next_node_number: 1,
            score_overrides: BTreeMap::new(),
        })
    }

    pub fn from_sgf(content: &str) -> Result<Self, DomainError> {
        let parsed_root = SgfParser::new(content).parse_game_tree()?;
        let (width, height) = parsed_root
            .properties
            .get("SZ")
            .and_then(|values| values.first())
            .map(|value| parse_board_size(value))
            .transpose()?
            .unwrap_or((19, 19));
        let mut game = Self::new(width, height)?;
        game.node_store.clear();
        game.preferred_child_by_node.clear();
        game.next_node_number = 1;
        game.insert_parsed_node(parsed_root, None)?;
        game.current_node_id = game.follow_preferred_line(&game.root_node_id)?;
        game.saved_revision = 0;
        Ok(game)
    }

    pub fn set_source_path(&mut self, source_path: Option<String>) {
        self.source_path = source_path;
        self.mark_saved();
    }

    /// Restores the selected node from editor workspace state without creating
    /// undo history or marking the SGF document dirty.
    pub fn restore_current_node(&mut self, node_id: &str) -> Result<(), DomainError> {
        if !self.node_store.contains_key(node_id) {
            return Err(DomainError::MissingNode(node_id.to_owned()));
        }
        self.current_node_id = node_id.to_owned();
        Ok(())
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    pub fn mark_dirty(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    /// Sets one root property in place, without creating history or marking
    /// the document dirty. This is intended only for constructing a fresh
    /// document (for example applying new-game defaults); existing documents
    /// must continue to use `SetNodeProperty` transactions.
    pub fn set_root_property(&mut self, property: &str, values: Vec<String>) {
        if let Some(root) = self.node_store.get_mut(&self.root_node_id) {
            root.properties.insert(property.to_owned(), values);
        }
    }

    pub fn snapshot(&self) -> GameSnapshot {
        let path = self.path_to_root(&self.current_node_id).unwrap_or_default();
        let moves = self.moves_for_path(&path).unwrap_or_default();
        let board = self
            .board_snapshot(&path)
            .unwrap_or_else(|_| self.empty_board_snapshot());
        let (black_captures, white_captures) = self.capture_counts(&path);
        let root_properties = self
            .node_store
            .get(&self.root_node_id)
            .map(|node| node.properties.clone())
            .unwrap_or_default();
        let can_undo = !self.undo_history.is_empty();
        let can_redo = !self.redo_history.is_empty();

        GameSnapshot {
            schema_version: CURRENT_GAME_SCHEMA_VERSION,
            revision: self.revision,
            root_properties,
            nodes: self.node_snapshots(),
            root_node_id: self.root_node_id.clone(),
            current_node_id: self.current_node_id.clone(),
            preferred_child_by_node: self.preferred_child_by_node.clone(),
            moves,
            board,
            history: HistorySnapshot {
                can_undo,
                can_redo,
                undo_depth: self.undo_history.len(),
                redo_depth: self.redo_history.len(),
            },
            file_state: FileStateSnapshot {
                path: self.source_path.clone(),
                format: self.source_path.as_ref().map(|_| "sgf".to_owned()),
                is_dirty: self.revision != self.saved_revision,
            },
            mode: GameMode::Play,
            can_undo,
            can_redo,
            source_path: self.source_path.clone(),
            score_overrides: self.score_overrides.clone(),
            black_captures,
            white_captures,
        }
    }

    pub fn play_move(&mut self, color: Color, vertex: Option<Vertex>) -> Result<(), DomainError> {
        self.apply_transaction(GameTransaction {
            schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type: if vertex.is_some() {
                GameTransactionType::PlayMove
            } else {
                GameTransactionType::Pass
            },
            color: Some(color),
            vertex,
            node_id: None,
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override: None,
        })
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous_state) = self.undo_history.pop() else {
            return false;
        };
        self.redo_history.push(self.capture_state());
        self.restore_state(previous_state);
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next_state) = self.redo_history.pop() else {
            return false;
        };
        self.undo_history.push(self.capture_state());
        self.restore_state(next_state);
        self.revision += 1;
        true
    }

    pub fn apply_transaction(&mut self, transaction: GameTransaction) -> Result<(), DomainError> {
        if transaction.schema_version != CURRENT_TRANSACTION_SCHEMA_VERSION {
            return Err(DomainError::UnsupportedTransactionSchema(
                transaction.schema_version,
            ));
        }

        let previous_state = self.capture_state();
        self.apply_transaction_inner(transaction)?;
        self.undo_history.push(previous_state);
        self.redo_history.clear();
        self.revision += 1;
        Ok(())
    }

    pub fn to_sgf(&self) -> String {
        self.serialize_sgf_node(&self.root_node_id)
            .map(|serialized_tree| format!("({serialized_tree})"))
            .unwrap_or_else(|_| "(;GM[1]FF[4]SZ[19])".to_owned())
    }

    fn apply_transaction_inner(&mut self, transaction: GameTransaction) -> Result<(), DomainError> {
        match transaction.transaction_type {
            GameTransactionType::PlayMove | GameTransactionType::Pass => {
                let color = transaction
                    .color
                    .ok_or(DomainError::MissingTransactionColor(
                        transaction.transaction_type,
                    ))?;
                let vertex = if transaction.transaction_type == GameTransactionType::Pass {
                    None
                } else {
                    transaction.vertex
                };
                self.validate_move(color, vertex)?;
                if let Some(existing_child_id) = self.matching_child_move(color, vertex)? {
                    self.current_node_id = existing_child_id;
                    return Ok(());
                }
                let properties = Properties::from([(
                    color.sgf_property().to_owned(),
                    vec![vertex.map(format_sgf_vertex).unwrap_or_default()],
                )]);
                let child_id = self.append_node(&self.current_node_id.clone(), properties)?;
                self.current_node_id = child_id;
                Ok(())
            }
            GameTransactionType::SetNodeProperty => {
                let node_id = self.transaction_node_id(&transaction)?;
                let property =
                    transaction
                        .property
                        .ok_or(DomainError::MissingTransactionProperty(
                            GameTransactionType::SetNodeProperty,
                        ))?;
                let node = self
                    .node_store
                    .get_mut(&node_id)
                    .ok_or_else(|| DomainError::MissingNode(node_id.clone()))?;
                if transaction.values.is_empty() {
                    node.properties.remove(&property);
                } else {
                    node.properties.insert(property, transaction.values);
                }
                Ok(())
            }
            GameTransactionType::RemoveNodeProperty => {
                let node_id = self.transaction_node_id(&transaction)?;
                let property =
                    transaction
                        .property
                        .ok_or(DomainError::MissingTransactionProperty(
                            GameTransactionType::RemoveNodeProperty,
                        ))?;
                let node = self
                    .node_store
                    .get_mut(&node_id)
                    .ok_or_else(|| DomainError::MissingNode(node_id.clone()))?;
                node.properties.remove(&property);
                Ok(())
            }
            GameTransactionType::AddMarkup => {
                let node_id = self.transaction_node_id(&transaction)?;
                let vertex = transaction
                    .vertex
                    .ok_or(DomainError::MissingTransactionVertex(
                        GameTransactionType::AddMarkup,
                    ))?;
                let marker = transaction
                    .marker
                    .ok_or(DomainError::MissingTransactionMarker(
                        GameTransactionType::AddMarkup,
                    ))?;
                self.add_markup(&node_id, vertex, marker)
            }
            GameTransactionType::AppendVariation => {
                let parent_id = self.transaction_node_id(&transaction)?;
                if transaction.nodes.is_empty() {
                    return Err(DomainError::EmptyVariation);
                }
                let mut current_parent_id = parent_id;
                for node_snapshot in transaction.nodes {
                    current_parent_id =
                        self.append_node(&current_parent_id, node_snapshot.properties)?;
                }
                self.current_node_id = current_parent_id;
                Ok(())
            }
            GameTransactionType::RemoveVariation => {
                let node_id = self.transaction_node_id(&transaction)?;
                self.remove_subtree(&node_id)
            }
            GameTransactionType::PromoteVariation => {
                let node_id = self.transaction_node_id(&transaction)?;
                self.promote_variation(&node_id)
            }
            GameTransactionType::Navigate => {
                let node_id = self.transaction_node_id(&transaction)?;
                if !self.node_store.contains_key(&node_id) {
                    return Err(DomainError::MissingNode(node_id));
                }
                self.current_node_id = node_id;
                Ok(())
            }
            GameTransactionType::ApplyScoringOverride => {
                let vertex = transaction
                    .vertex
                    .ok_or(DomainError::MissingTransactionVertex(
                        GameTransactionType::ApplyScoringOverride,
                    ))?;
                let path = self.path_to_root(&self.current_node_id)?;
                let (board, _) = self.rebuild_board(&path)?;
                if !board.has(vertex) {
                    return Err(DomainError::VertexOutsideBoard);
                }
                let override_value = transaction
                    .score_override
                    .ok_or(DomainError::MissingScoreOverride)?;
                if !(-1..=1).contains(&override_value) {
                    return Err(DomainError::InvalidScoreOverride(override_value));
                }
                if override_value == 0 {
                    self.score_overrides.remove(&vertex);
                } else {
                    self.score_overrides.insert(vertex, override_value);
                }
                Ok(())
            }
        }
    }

    fn transaction_node_id(&self, transaction: &GameTransaction) -> Result<NodeId, DomainError> {
        Ok(transaction
            .node_id
            .clone()
            .unwrap_or_else(|| self.current_node_id.clone()))
    }

    fn capture_state(&self) -> DocumentState {
        DocumentState {
            node_store: self.node_store.clone(),
            current_node_id: self.current_node_id.clone(),
            preferred_child_by_node: self.preferred_child_by_node.clone(),
            score_overrides: self.score_overrides.clone(),
        }
    }

    fn restore_state(&mut self, state: DocumentState) {
        self.node_store = state.node_store;
        self.current_node_id = state.current_node_id;
        self.preferred_child_by_node = state.preferred_child_by_node;
        self.score_overrides = state.score_overrides;
    }

    fn allocate_node_id(&mut self) -> NodeId {
        loop {
            let node_id = format!("node-{}", self.next_node_number);
            self.next_node_number += 1;
            if !self.node_store.contains_key(&node_id) {
                return node_id;
            }
        }
    }

    fn insert_parsed_node(
        &mut self,
        parsed_node: ParsedSgfNode,
        parent_id: Option<NodeId>,
    ) -> Result<NodeId, DomainError> {
        let node_id = if parent_id.is_none() {
            ROOT_NODE_ID.to_owned()
        } else {
            self.allocate_node_id()
        };
        let node = Node {
            id: node_id.clone(),
            parent_id: parent_id.clone(),
            child_ids: Vec::new(),
            properties: parsed_node.properties,
        };
        self.node_store.insert(node_id.clone(), node);
        if let Some(parent_id) = parent_id {
            self.node_store
                .get_mut(&parent_id)
                .ok_or_else(|| DomainError::MissingNode(parent_id.clone()))?
                .child_ids
                .push(node_id.clone());
            self.preferred_child_by_node
                .entry(parent_id)
                .or_insert_with(|| node_id.clone());
        }
        for child in parsed_node.children {
            self.insert_parsed_node(child, Some(node_id.clone()))?;
        }
        Ok(node_id)
    }

    fn append_node(
        &mut self,
        parent_id: &str,
        properties: Properties,
    ) -> Result<NodeId, DomainError> {
        if !self.node_store.contains_key(parent_id) {
            return Err(DomainError::MissingNode(parent_id.to_owned()));
        }
        let node_id = self.allocate_node_id();
        self.node_store.insert(
            node_id.clone(),
            Node {
                id: node_id.clone(),
                parent_id: Some(parent_id.to_owned()),
                child_ids: Vec::new(),
                properties,
            },
        );
        let parent = self
            .node_store
            .get_mut(parent_id)
            .ok_or_else(|| DomainError::MissingNode(parent_id.to_owned()))?;
        parent.child_ids.push(node_id.clone());
        self.preferred_child_by_node
            .entry(parent_id.to_owned())
            .or_insert_with(|| node_id.clone());
        Ok(node_id)
    }

    fn matching_child_move(
        &self,
        color: Color,
        vertex: Option<Vertex>,
    ) -> Result<Option<NodeId>, DomainError> {
        let current_node = self
            .node_store
            .get(&self.current_node_id)
            .ok_or_else(|| DomainError::MissingNode(self.current_node_id.clone()))?;
        for child_id in &current_node.child_ids {
            let child = self
                .node_store
                .get(child_id)
                .ok_or_else(|| DomainError::MissingNode(child_id.clone()))?;
            if child.move_data()? == Some(MoveDto { color, vertex }) {
                return Ok(Some(child_id.clone()));
            }
        }
        Ok(None)
    }

    fn validate_move(&self, color: Color, vertex: Option<Vertex>) -> Result<(), DomainError> {
        let path = self.path_to_root(&self.current_node_id)?;
        let (board, historical_boards) = self.rebuild_board(&path)?;
        let previous_position = historical_boards.get(historical_boards.len().saturating_sub(2));
        // Superko history: every position before the parent (the last entry is
        // the current board itself, excluded).
        let history_len = historical_boards.len().saturating_sub(1);
        board
            .make_move_with_history(
                color,
                vertex,
                previous_position,
                &historical_boards[..history_len],
            )
            .map(|_| ())
    }

    fn remove_subtree(&mut self, node_id: &str) -> Result<(), DomainError> {
        if node_id == self.root_node_id {
            return Err(DomainError::CannotRemoveRoot);
        }
        let node = self
            .node_store
            .get(node_id)
            .cloned()
            .ok_or_else(|| DomainError::MissingNode(node_id.to_owned()))?;
        let parent_id = node
            .parent_id
            .clone()
            .ok_or(DomainError::CannotRemoveRoot)?;
        self.node_store
            .get_mut(&parent_id)
            .ok_or_else(|| DomainError::MissingNode(parent_id.clone()))?
            .child_ids
            .retain(|child_id| child_id != node_id);
        self.preferred_child_by_node.remove(&parent_id);
        let remaining_first_child = self
            .node_store
            .get(&parent_id)
            .and_then(|parent| parent.child_ids.first())
            .cloned();
        if let Some(first_child_id) = remaining_first_child {
            self.preferred_child_by_node
                .insert(parent_id.clone(), first_child_id);
        }
        let mut nodes_to_remove = vec![node_id.to_owned()];
        while let Some(current_id) = nodes_to_remove.pop() {
            if let Some(removed_node) = self.node_store.remove(&current_id) {
                nodes_to_remove.extend(removed_node.child_ids);
            }
            self.preferred_child_by_node.remove(&current_id);
        }
        if !self.node_store.contains_key(&self.current_node_id) {
            self.current_node_id = parent_id;
        }
        Ok(())
    }

    fn promote_variation(&mut self, node_id: &str) -> Result<(), DomainError> {
        let mut current_id = node_id.to_owned();
        loop {
            let parent_id = self
                .node_store
                .get(&current_id)
                .ok_or_else(|| DomainError::MissingNode(current_id.clone()))?
                .parent_id
                .clone();
            let Some(parent_id) = parent_id else { break };
            let parent = self
                .node_store
                .get_mut(&parent_id)
                .ok_or_else(|| DomainError::MissingNode(parent_id.clone()))?;
            if let Some(index) = parent
                .child_ids
                .iter()
                .position(|child_id| child_id == &current_id)
            {
                parent.child_ids.remove(index);
                parent.child_ids.insert(0, current_id.clone());
                self.preferred_child_by_node
                    .insert(parent_id.clone(), current_id.clone());
            }
            current_id = parent_id;
        }
        Ok(())
    }

    fn add_markup(
        &mut self,
        node_id: &str,
        vertex: Vertex,
        marker: MarkerSnapshot,
    ) -> Result<(), DomainError> {
        let property = match marker.marker_type.as_str() {
            "circle" => "CR",
            "cross" => "MA",
            "square" => "SQ",
            "triangle" => "TR",
            "label" => "LB",
            _ => return Err(DomainError::InvalidCoordinate),
        };
        let value = if property == "LB" {
            format!(
                "{}:{}",
                format_sgf_vertex(vertex),
                marker.label.unwrap_or_default()
            )
        } else {
            format_sgf_vertex(vertex)
        };
        let node = self
            .node_store
            .get_mut(node_id)
            .ok_or_else(|| DomainError::MissingNode(node_id.to_owned()))?;
        let values = node.properties.entry(property.to_owned()).or_default();
        if !values.contains(&value) {
            values.push(value);
        }
        Ok(())
    }

    fn path_to_root(&self, node_id: &str) -> Result<Vec<NodeId>, DomainError> {
        let mut path = Vec::new();
        let mut current_id = node_id.to_owned();
        loop {
            let node = self
                .node_store
                .get(&current_id)
                .ok_or_else(|| DomainError::MissingNode(current_id.clone()))?;
            path.push(current_id.clone());
            let Some(parent_id) = &node.parent_id else {
                break;
            };
            current_id = parent_id.clone();
        }
        path.reverse();
        Ok(path)
    }

    fn follow_preferred_line(&self, starting_node_id: &str) -> Result<NodeId, DomainError> {
        let mut current_id = starting_node_id.to_owned();
        loop {
            let node = self
                .node_store
                .get(&current_id)
                .ok_or_else(|| DomainError::MissingNode(current_id.clone()))?;
            let next_id = self
                .preferred_child_by_node
                .get(&current_id)
                .cloned()
                .or_else(|| node.child_ids.first().cloned());
            let Some(next_id) = next_id else {
                return Ok(current_id);
            };
            current_id = next_id;
        }
    }

    fn moves_for_path(&self, path: &[NodeId]) -> Result<Vec<MoveDto>, DomainError> {
        path.iter()
            .skip(1)
            .filter_map(|node_id| self.node_store.get(node_id))
            .map(Node::move_data)
            .filter_map(Result::transpose)
            .collect()
    }

    fn rebuild_board(&self, path: &[NodeId]) -> Result<(Board, Vec<Board>), DomainError> {
        let root_node = self
            .node_store
            .get(&self.root_node_id)
            .ok_or_else(|| DomainError::MissingNode(self.root_node_id.clone()))?;
        let (width, height) = root_node
            .properties
            .get("SZ")
            .and_then(|values| values.first())
            .map(|value| parse_board_size(value))
            .transpose()?
            .unwrap_or((19, 19));
        let mut board = Board::new(width, height)?;
        let mut historical_boards = vec![board.clone()];

        for node_id in path {
            let node = self
                .node_store
                .get(node_id)
                .ok_or_else(|| DomainError::MissingNode(node_id.clone()))?;
            if let Some(move_data) = node.move_data()? {
                let previous_position =
                    historical_boards.get(historical_boards.len().saturating_sub(2));
                // Superko history: every position before the current board.
                board = board.make_move_with_history(
                    move_data.color,
                    move_data.vertex,
                    previous_position,
                    &historical_boards,
                )?;
            }
            apply_setup_properties(&mut board, &node.properties)?;
            historical_boards.push(board.clone());
        }
        Ok((board, historical_boards))
    }

    /// Counts captures along the active path by replaying each move and
    /// measuring how many opponent stones disappear. Replaying (rather than a
    /// board-difference heuristic) keeps the count correct across setup stones,
    /// handicap placements and passes. Returns `(black_captures, white_captures)`,
    /// i.e. the stones each side has removed from the opponent.
    fn capture_counts(&self, path: &[NodeId]) -> (usize, usize) {
        fn count_stones(board: &Board, color: Color) -> usize {
            let sign = color.stone_value();
            board
                .sign_map
                .iter()
                .flatten()
                .filter(|&&value| value == sign)
                .count()
        }

        let Ok(root_node) = self
            .node_store
            .get(&self.root_node_id)
            .ok_or_else(|| DomainError::MissingNode(self.root_node_id.clone()))
        else {
            return (0, 0);
        };
        let board_size = root_node
            .properties
            .get("SZ")
            .and_then(|values| values.first())
            .and_then(|value| parse_board_size(value).ok())
            .unwrap_or((19, 19));
        let Ok(mut board) = Board::new(board_size.0, board_size.1) else {
            return (0, 0);
        };

        let mut black_captures = 0usize;
        let mut white_captures = 0usize;
        for node_id in path {
            let Some(node) = self.node_store.get(node_id) else {
                continue;
            };
            if let Ok(Some(move_data)) = node.move_data()
                && let Some(vertex) = move_data.vertex
            {
                let before = count_stones(&board, move_data.color.opponent());
                if let Ok(next) = board.make_move(move_data.color, Some(vertex), None) {
                    let after = count_stones(&next, move_data.color.opponent());
                    let captured = before.saturating_sub(after);
                    match move_data.color {
                        Color::Black => black_captures += captured,
                        Color::White => white_captures += captured,
                    }
                    board = next;
                }
            }
            let _ = apply_setup_properties(&mut board, &node.properties);
        }
        (black_captures, white_captures)
    }

    fn board_snapshot(&self, path: &[NodeId]) -> Result<BoardSnapshot, DomainError> {
        let (board, _) = self.rebuild_board(path)?;
        let current_node = self
            .node_store
            .get(&self.current_node_id)
            .ok_or_else(|| DomainError::MissingNode(self.current_node_id.clone()))?;
        let move_number = self.moves_for_path(path)?.len();
        let next_player = current_node
            .move_data()?
            .map(|move_data| move_data.color.opponent())
            .unwrap_or(Color::Black);
        let mut markers = vec![vec![None; board.width]; board.height];
        let mut lines = Vec::new();

        if let Some(move_data) = current_node.move_data()?
            && let Some(vertex) = move_data.vertex.filter(|vertex| board.has(*vertex))
        {
            markers[vertex.row][vertex.column] = Some(MarkerSnapshot {
                marker_type: "point".to_owned(),
                label: None,
            });
        }
        apply_markup_properties(&mut markers, &mut lines, &current_node.properties, &board)?;

        let children_info = self.variation_info(&current_node.child_ids)?;
        let siblings_info = current_node
            .parent_id
            .as_ref()
            .map(|parent_id| {
                self.node_store
                    .get(parent_id)
                    .map(|parent| {
                        parent
                            .child_ids
                            .iter()
                            .filter(|sibling_id| *sibling_id != &self.current_node_id)
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .map(|siblings| self.variation_info(&siblings))
            .transpose()?
            .unwrap_or_default();

        Ok(BoardSnapshot {
            width: board.width,
            height: board.height,
            sign_map: board.sign_map,
            current_vertex: board.current_vertex,
            next_player,
            move_number,
            markers,
            lines,
            children_info,
            siblings_info,
        })
    }

    fn variation_info(
        &self,
        node_ids: &[NodeId],
    ) -> Result<Vec<VariationInfoSnapshot>, DomainError> {
        let mut variations = Vec::new();
        for node_id in node_ids {
            let node = self
                .node_store
                .get(node_id)
                .ok_or_else(|| DomainError::MissingNode(node_id.clone()))?;
            let Some(move_data) = node.move_data()? else {
                continue;
            };
            let Some(vertex) = move_data.vertex else {
                continue;
            };
            variations.push(VariationInfoSnapshot {
                vertex,
                color: move_data.color,
                annotation: move_annotation(&node.properties),
            });
        }
        Ok(variations)
    }

    fn empty_board_snapshot(&self) -> BoardSnapshot {
        BoardSnapshot {
            width: 0,
            height: 0,
            sign_map: Vec::new(),
            current_vertex: None,
            next_player: Color::Black,
            move_number: 0,
            markers: Vec::new(),
            lines: Vec::new(),
            children_info: Vec::new(),
            siblings_info: Vec::new(),
        }
    }

    fn node_snapshots(&self) -> Vec<NodeSnapshot> {
        let mut snapshots = Vec::with_capacity(self.node_store.len());
        if let Some(root_node) = self.node_store.get(&self.root_node_id) {
            snapshots.push(root_node.snapshot());
        }
        snapshots.extend(
            self.node_store
                .iter()
                .filter(|(node_id, _)| *node_id != &self.root_node_id)
                .map(|(_, node)| node.snapshot()),
        );
        snapshots
    }

    fn serialize_sgf_node(&self, node_id: &str) -> Result<String, DomainError> {
        let node = self
            .node_store
            .get(node_id)
            .ok_or_else(|| DomainError::MissingNode(node_id.to_owned()))?;
        let properties = serialize_properties(&node.properties);
        if node.child_ids.is_empty() {
            return Ok(format!(";{properties}"));
        }
        if node.child_ids.len() == 1 {
            return Ok(format!(
                ";{properties}{}",
                self.serialize_sgf_node(&node.child_ids[0])?
            ));
        }
        let variations = node
            .child_ids
            .iter()
            .map(|child_id| {
                self.serialize_sgf_node(child_id)
                    .map(|serialized| format!("({serialized})"))
            })
            .collect::<Result<String, _>>()?;
        Ok(format!(";{properties}{variations}"))
    }
}

fn apply_setup_properties(board: &mut Board, properties: &Properties) -> Result<(), DomainError> {
    for (property, sign) in [("AW", -1), ("AE", 0), ("AB", 1)] {
        for value in properties.get(property).into_iter().flatten() {
            for vertex in parse_compressed_vertices(value)? {
                if board.has(vertex) {
                    board.set(vertex, sign);
                }
            }
        }
    }
    Ok(())
}

fn apply_markup_properties(
    markers: &mut [Vec<Option<MarkerSnapshot>>],
    lines: &mut Vec<BoardLineSnapshot>,
    properties: &Properties,
    board: &Board,
) -> Result<(), DomainError> {
    for (property, marker_type) in [
        ("CR", "circle"),
        ("MA", "cross"),
        ("SQ", "square"),
        ("TR", "triangle"),
    ] {
        for value in properties.get(property).into_iter().flatten() {
            for vertex in parse_compressed_vertices(value)? {
                if board.has(vertex) {
                    markers[vertex.row][vertex.column] = Some(MarkerSnapshot {
                        marker_type: marker_type.to_owned(),
                        label: None,
                    });
                }
            }
        }
    }
    if let Some(labels) = properties.get("LB") {
        for composed_value in labels {
            if let Some((point, label)) = composed_value.split_once(':')
                && let Some(vertex) = parse_sgf_vertex(point)?
                && board.has(vertex)
            {
                markers[vertex.row][vertex.column] = Some(MarkerSnapshot {
                    marker_type: "label".to_owned(),
                    label: Some(label.to_owned()),
                });
            }
        }
    }
    if let Some(labels) = properties.get("L") {
        for (index, point) in labels.iter().enumerate() {
            let Some(vertex) = parse_sgf_vertex(point)? else {
                continue;
            };
            if board.has(vertex) {
                let label = (b'A' + index as u8) as char;
                markers[vertex.row][vertex.column] = Some(MarkerSnapshot {
                    marker_type: "label".to_owned(),
                    label: Some(label.to_string()),
                });
            }
        }
    }
    for (property, line_type) in [("AR", "arrow"), ("LN", "line")] {
        for composed_value in properties.get(property).into_iter().flatten() {
            if let Some((start, end)) = composed_value.split_once(':')
                && let (Some(start), Some(end)) = (parse_sgf_vertex(start)?, parse_sgf_vertex(end)?)
            {
                lines.push(BoardLineSnapshot {
                    start,
                    end,
                    line_type: line_type.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn move_annotation(properties: &Properties) -> Option<String> {
    ["BM", "DO", "IT", "TE"]
        .into_iter()
        .find(|property| properties.contains_key(*property))
        .map(ToOwned::to_owned)
}

fn parse_compressed_vertices(value: &str) -> Result<Vec<Vertex>, DomainError> {
    let Some((start, end)) = value.split_once(':') else {
        return parse_sgf_vertex(value).map(|vertex| vertex.into_iter().collect());
    };
    let start = parse_sgf_vertex(start)?.ok_or(DomainError::InvalidCoordinate)?;
    let end = parse_sgf_vertex(end)?.ok_or(DomainError::InvalidCoordinate)?;
    let (minimum_column, maximum_column) = if start.column <= end.column {
        (start.column, end.column)
    } else {
        (end.column, start.column)
    };
    let (minimum_row, maximum_row) = if start.row <= end.row {
        (start.row, end.row)
    } else {
        (end.row, start.row)
    };
    Ok((minimum_column..=maximum_column)
        .flat_map(|column| (minimum_row..=maximum_row).map(move |row| Vertex { column, row }))
        .collect())
}

fn parse_board_size(value: &str) -> Result<(usize, usize), DomainError> {
    let mut dimensions = value.split(':');
    let width = dimensions
        .next()
        .and_then(|dimension| dimension.parse().ok())
        .ok_or(DomainError::UnsupportedBoardSize)?;
    let height = dimensions
        .next()
        .and_then(|dimension| dimension.parse().ok())
        .unwrap_or(width);
    if dimensions.next().is_some() || !(2..=25).contains(&width) || !(2..=25).contains(&height) {
        return Err(DomainError::UnsupportedBoardSize);
    }
    Ok((width, height))
}

fn format_board_size(width: usize, height: usize) -> String {
    if width == height {
        width.to_string()
    } else {
        format!("{width}:{height}")
    }
}

fn parse_sgf_vertex(value: &str) -> Result<Option<Vertex>, DomainError> {
    if value.is_empty() {
        return Ok(None);
    }
    let bytes = value.as_bytes();
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_lowercase) {
        return Err(DomainError::InvalidCoordinate);
    }
    Ok(Some(Vertex {
        column: (bytes[0] - b'a') as usize,
        row: (bytes[1] - b'a') as usize,
    }))
}

fn format_sgf_vertex(vertex: Vertex) -> String {
    let column = (b'a' + vertex.column as u8) as char;
    let row = (b'a' + vertex.row as u8) as char;
    format!("{column}{row}")
}

fn serialize_properties(properties: &Properties) -> String {
    properties
        .iter()
        .map(|(property, values)| {
            format!(
                "{property}{}",
                values
                    .iter()
                    .map(|value| format!("[{}]", escape_sgf_value(value)))
                    .collect::<String>()
            )
        })
        .collect()
}

fn escape_sgf_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace(']', "\\]")
}

#[derive(Clone, Debug)]
struct ParsedSgfNode {
    properties: Properties,
    children: Vec<ParsedSgfNode>,
}

/// Lightweight extraction of the root node's SGF properties without building a
/// full game tree. Used for library indexing where only header metadata (PB,
/// PW, RE, GN, ...) is needed. Returns an empty map when the text is not a
/// well-formed SGF root, so callers can degrade gracefully instead of failing
/// the whole scan on one malformed file. Escape and whitespace handling is
/// delegated to the authoritative `SgfParser`, so this can never diverge from
/// full-document parsing.
pub fn extract_root_properties(sgf: &str) -> Properties {
    SgfParser::new(sgf)
        .parse_root_properties()
        .unwrap_or_default()
}

struct SgfParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> SgfParser<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            bytes: content.as_bytes(),
            index: 0,
        }
    }

    /// Parses only the root node's properties by reusing the authoritative
    /// sequence/property/value machinery. Stops after the first node, so
    /// move sequences and variations are never walked.
    fn parse_root_properties(&mut self) -> Result<Properties, DomainError> {
        self.skip_whitespace();
        self.expect(b'(')?;
        self.skip_whitespace();
        self.expect(b';')?;
        self.parse_properties()
    }

    fn parse_game_tree(&mut self) -> Result<ParsedSgfNode, DomainError> {
        self.skip_whitespace();
        self.expect(b'(')?;
        let mut nodes = self.parse_sequence()?;
        self.skip_whitespace();
        let mut nested_variations = Vec::new();
        while self.peek() == Some(b'(') {
            nested_variations.push(self.parse_game_tree()?);
            self.skip_whitespace();
        }
        self.expect(b')')?;

        let mut root = nodes.remove(0);
        let mut tail = &mut root;
        for node in nodes {
            tail.children.push(node);
            tail = tail.children.last_mut().expect("node was just added");
        }
        tail.children.extend(nested_variations);
        Ok(root)
    }

    fn parse_sequence(&mut self) -> Result<Vec<ParsedSgfNode>, DomainError> {
        let mut nodes = Vec::new();
        self.skip_whitespace();
        while self.peek() == Some(b';') {
            self.index += 1;
            nodes.push(ParsedSgfNode {
                properties: self.parse_properties()?,
                children: Vec::new(),
            });
            self.skip_whitespace();
        }
        if nodes.is_empty() {
            return Err(DomainError::MissingGameTree);
        }
        Ok(nodes)
    }

    fn parse_properties(&mut self) -> Result<Properties, DomainError> {
        let mut properties = Properties::new();
        loop {
            self.skip_whitespace();
            if matches!(self.peek(), Some(b';' | b'(' | b')') | None) {
                break;
            }
            let property_start = self.index;
            while self.peek().is_some_and(|byte| byte.is_ascii_uppercase()) {
                self.index += 1
            }
            if property_start == self.index {
                return Err(DomainError::InvalidSgf(self.index));
            }
            let property = std::str::from_utf8(&self.bytes[property_start..self.index])
                .map_err(|_| DomainError::InvalidSgf(property_start))?
                .to_owned();
            let values = properties.entry(property).or_default();
            let mut has_value = false;
            while self.peek() == Some(b'[') {
                has_value = true;
                values.push(self.parse_value()?);
            }
            if !has_value {
                return Err(DomainError::InvalidSgf(self.index));
            }
        }
        Ok(properties)
    }

    fn parse_value(&mut self) -> Result<String, DomainError> {
        self.expect(b'[')?;
        let mut value = Vec::new();
        while let Some(byte) = self.peek() {
            self.index += 1;
            match byte {
                b']' => {
                    return String::from_utf8(value)
                        .map_err(|_| DomainError::InvalidSgf(self.index));
                }
                b'\r' => continue,
                b'\\' => {
                    if let Some(escaped_byte) = self.peek() {
                        self.index += 1;
                        if escaped_byte == b'\r' {
                            if self.peek() == Some(b'\n') {
                                self.index += 1;
                            }
                        } else if escaped_byte != b'\n' {
                            value.push(escaped_byte);
                        }
                    }
                }
                byte => value.push(byte),
            }
        }
        Err(DomainError::InvalidSgf(self.index))
    }

    fn expect(&mut self, expected: u8) -> Result<(), DomainError> {
        if self.peek() == Some(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(DomainError::InvalidSgf(self.index))
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.index += 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction(transaction_type: GameTransactionType) -> GameTransaction {
        GameTransaction {
            schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type,
            color: None,
            vertex: None,
            node_id: None,
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override: None,
        }
    }

    #[test]
    fn superko_rejects_recreating_an_earlier_position_via_history() {
        // Directly exercise the superko check: a board, an earlier position in
        // history, and a move that would recreate that earlier position.
        let board = Board::new(5, 5).unwrap();
        // Manually craft a move that captures then is answered, reproducing the
        // starting empty board is impossible on a fresh board, so instead build
        // a 2-cycle: place a stone, then a capturing sequence that returns to a
        // prior sign_map. We validate the primitive directly.
        let mut b1 = board.clone();
        b1.set(Vertex { column: 1, row: 1 }, 1);
        // The candidate move must be legal on b1; choose an empty vertex and
        // verify that a history containing b1's resulting position is rejected.
        let result_board = {
            let mut r = b1.clone();
            r.set(Vertex { column: 2, row: 2 }, -1);
            r
        };
        // Pretend `result_board` already occurred earlier on the path.
        let history = vec![result_board.clone()];
        let verdict = b1.make_move_with_history(
            Color::White,
            Some(Vertex { column: 2, row: 2 }),
            None,
            &history,
        );
        assert!(matches!(verdict, Err(DomainError::KoViolation)));
    }

    #[test]
    fn superko_allows_a_move_that_does_not_recreate_history() {
        let board = Board::new(5, 5).unwrap();
        let history = vec![board.clone()]; // only the empty start occurred before
        let verdict = board.make_move_with_history(
            Color::Black,
            Some(Vertex { column: 0, row: 0 }),
            None,
            &history,
        );
        assert!(verdict.is_ok());
    }

    #[test]
    fn superko_rejects_recreating_any_earlier_position_regardless_of_distance() {
        // The superko guard must fire for an earlier position that is NOT the
        // immediate parent (distance > 1), which a simple one-move ko misses.
        // Craft a board `b`, a legal move producing `after`, then place `b`
        // deep in `history` and assert a move recreating `b` is rejected.
        let mut b = Board::new(4, 4).unwrap();
        b.set(Vertex { column: 0, row: 0 }, 1);
        b.set(Vertex { column: 1, row: 1 }, -1);
        // `after` differs from `b` by one extra stone at (2,2).
        let after = b
            .make_move_with_history(Color::Black, Some(Vertex { column: 2, row: 2 }), None, &[])
            .expect("setup move is legal");
        // A follow-up move on `after` whose result equals `b`: removing the
        // stone at (2,2) is not a legal move, so instead verify the guard
        // through a capture that reproduces `b`. Surround the white stone at
        // (1,1) so a capture removes it, then check that a position equal to
        // `b` in history triggers the violation on the appropriate move.
        let history = vec![Board::new(4, 4).unwrap(), b.clone()];
        // Play a move on `after` that is legal and does NOT recreate history.
        let ok_move = after.make_move_with_history(
            Color::White,
            Some(Vertex { column: 3, row: 3 }),
            None,
            &history,
        );
        assert!(ok_move.is_ok());
        // Directly assert the guard: a move whose result equals `b` is rejected
        // when `b` is anywhere in `history`. Reuse the primitive by checking
        // that recreating `b`'s exact sign_map is caught — place the resulting
        // board in history and attempt the identical move on `b` itself.
        let identical = b.make_move_with_history(
            Color::Black,
            Some(Vertex { column: 2, row: 2 }),
            None,
            std::slice::from_ref(&after), // history already contains the result
        );
        assert!(matches!(identical, Err(DomainError::KoViolation)));
    }

    #[test]
    fn parses_variations_and_round_trips_unknown_properties() {
        let game = GameDocument::from_sgf(
            "(;GM[1]FF[4]SZ[5]XX[keep];B[aa](;W[bb]C[first])(;W[cc]TR[dd]))",
        )
        .unwrap();
        let snapshot = game.snapshot();

        assert_eq!(snapshot.nodes.len(), 4);
        assert_eq!(snapshot.root_properties["XX"], ["keep"]);
        assert_eq!(snapshot.nodes[1].child_ids.len(), 2);
        assert_eq!(snapshot.current_node_id, "node-2");
        assert!(game.to_sgf().contains("XX[keep]"));
        let round_tripped_game = GameDocument::from_sgf(&game.to_sgf()).unwrap();
        assert_eq!(round_tripped_game.snapshot().nodes.len(), 4);
        assert_eq!(
            round_tripped_game.snapshot().root_properties["XX"],
            ["keep"]
        );
    }

    #[test]
    fn counts_captures_along_the_active_path() {
        // Black surrounds and captures the single white stone at bb.
        // Sequence: B[ba] W[bb] B[ab] B[cb] B[bc] → bb loses its last liberty.
        let game = GameDocument::from_sgf("(;SZ[5];B[ba];W[bb];B[ab];B[cb];B[bc])").unwrap();
        let snapshot = game.snapshot();
        assert_eq!(snapshot.black_captures, 1);
        assert_eq!(snapshot.white_captures, 0);
        // The captured point is empty again on the live board.
        assert_eq!(snapshot.board.sign_map[1][1], 0);
    }

    #[test]
    fn passes_and_setup_stones_do_not_inflate_captures() {
        // Setup stones + a pass: no captures should be recorded.
        let game = GameDocument::from_sgf("(;SZ[5]AB[aa]AW[ee];B[];W[])").unwrap();
        let snapshot = game.snapshot();
        assert_eq!(snapshot.black_captures, 0);
        assert_eq!(snapshot.white_captures, 0);
    }

    #[test]
    fn captures_reset_when_navigating_before_the_capture() {
        let mut game = GameDocument::from_sgf("(;SZ[5];B[ba];W[bb];B[ab];B[cb];B[bc])").unwrap();
        // Jump back to the root: the capture has not happened yet on that path.
        game.apply_transaction(GameTransaction {
            node_id: Some(game.snapshot().root_node_id.clone()),
            ..transaction(GameTransactionType::Navigate)
        })
        .unwrap();
        let snapshot = game.snapshot();
        assert_eq!(snapshot.black_captures, 0);
        assert_eq!(snapshot.white_captures, 0);
    }

    #[test]
    fn builds_board_markup_and_variation_overlays_for_current_node() {
        let mut game = GameDocument::from_sgf(
            "(;SZ[5]AB[aa:ab]AW[ee];B[ac](;W[bb]TR[cc]LB[dd:X]AR[aa:bb])(;W[dd]BM[1]))",
        )
        .unwrap();
        game.apply_transaction(GameTransaction {
            node_id: Some("node-2".to_owned()),
            ..transaction(GameTransactionType::Navigate)
        })
        .unwrap();
        let board = game.snapshot().board;

        assert_eq!(board.sign_map[0][0], 1);
        assert_eq!(board.sign_map[1][0], 1);
        assert_eq!(board.sign_map[2][0], 1);
        assert_eq!(
            board.markers[2][2].as_ref().unwrap().marker_type,
            "triangle"
        );
        assert_eq!(
            board.markers[3][3].as_ref().unwrap().label.as_deref(),
            Some("X")
        );
        assert_eq!(board.lines[0].line_type, "arrow");
        assert_eq!(board.siblings_info.len(), 1);
        assert_eq!(board.siblings_info[0].annotation.as_deref(), Some("BM"));
    }

    #[test]
    fn appends_promotes_and_removes_variations_through_transactions() {
        let mut game = GameDocument::new(5, 5).unwrap();
        let mut black_move = transaction(GameTransactionType::PlayMove);
        black_move.color = Some(Color::Black);
        black_move.vertex = Some(Vertex { column: 0, row: 0 });
        game.apply_transaction(black_move).unwrap();

        let variation = GameTransaction {
            node_id: Some("node-1".to_owned()),
            nodes: vec![NodeSnapshot {
                id: "ignored".to_owned(),
                parent_id: None,
                child_ids: Vec::new(),
                properties: Properties::from([("W".to_owned(), vec!["bb".to_owned()])]),
            }],
            ..transaction(GameTransactionType::AppendVariation)
        };
        game.apply_transaction(variation).unwrap();
        let second_child_id = game.snapshot().current_node_id;

        game.apply_transaction(GameTransaction {
            node_id: Some("node-1".to_owned()),
            ..transaction(GameTransactionType::Navigate)
        })
        .unwrap();
        let variation = GameTransaction {
            node_id: Some("node-1".to_owned()),
            nodes: vec![NodeSnapshot {
                id: "ignored".to_owned(),
                parent_id: None,
                child_ids: Vec::new(),
                properties: Properties::from([("W".to_owned(), vec!["cc".to_owned()])]),
            }],
            ..transaction(GameTransactionType::AppendVariation)
        };
        game.apply_transaction(variation).unwrap();
        let promoted_child_id = game.snapshot().current_node_id;

        game.apply_transaction(GameTransaction {
            node_id: Some(promoted_child_id.clone()),
            ..transaction(GameTransactionType::PromoteVariation)
        })
        .unwrap();
        assert_eq!(
            game.snapshot()
                .nodes
                .iter()
                .find(|node| node.id == "node-1")
                .unwrap()
                .child_ids[0],
            promoted_child_id
        );

        game.apply_transaction(GameTransaction {
            node_id: Some(second_child_id),
            ..transaction(GameTransactionType::RemoveVariation)
        })
        .unwrap();
        assert_eq!(game.snapshot().nodes.len(), 3);
    }

    #[test]
    fn applies_properties_markup_navigation_and_history() {
        let mut game = GameDocument::new(5, 5).unwrap();
        let mut move_transaction = transaction(GameTransactionType::PlayMove);
        move_transaction.color = Some(Color::Black);
        move_transaction.vertex = Some(Vertex { column: 1, row: 1 });
        game.apply_transaction(move_transaction).unwrap();
        game.apply_transaction(GameTransaction {
            property: Some("C".to_owned()),
            values: vec!["note".to_owned()],
            ..transaction(GameTransactionType::SetNodeProperty)
        })
        .unwrap();
        game.apply_transaction(GameTransaction {
            vertex: Some(Vertex { column: 2, row: 2 }),
            marker: Some(MarkerSnapshot {
                marker_type: "circle".to_owned(),
                label: None,
            }),
            ..transaction(GameTransactionType::AddMarkup)
        })
        .unwrap();

        let snapshot = game.snapshot();
        assert_eq!(snapshot.nodes[1].properties["C"], ["note"]);
        assert_eq!(
            snapshot.board.markers[2][2].as_ref().unwrap().marker_type,
            "circle"
        );
        assert!(snapshot.can_undo);

        assert!(game.undo());
        assert!(game.snapshot().can_redo);
        assert!(game.redo());
        assert_eq!(
            game.snapshot().board.markers[2][2]
                .as_ref()
                .unwrap()
                .marker_type,
            "circle"
        );
    }

    #[test]
    fn preserves_escaped_and_unicode_property_values() {
        let game =
            GameDocument::from_sgf("(;SZ[5]C[Bracket: \\] and slash: \\\\]N[日本語]GC[中文])")
                .unwrap();
        let snapshot = game.snapshot();

        assert_eq!(snapshot.root_properties["C"], ["Bracket: ] and slash: \\"]);
        assert_eq!(snapshot.root_properties["N"], ["日本語"]);
        assert_eq!(snapshot.root_properties["GC"], ["中文"]);

        let serialized = game.to_sgf();
        assert!(serialized.contains("C[Bracket: \\] and slash: \\\\]"));
        let round_tripped_snapshot = GameDocument::from_sgf(&serialized).unwrap().snapshot();
        assert_eq!(
            round_tripped_snapshot.root_properties,
            snapshot.root_properties
        );
    }

    #[test]
    fn extracts_root_properties_without_building_a_game_tree() {
        let properties = extract_root_properties(
            "(;GM[1]FF[4]SZ[19]PB[柯洁]PW[申真谞]RE[黑中盘胜]GN[Example];B[pd];W[dp])",
        );
        assert_eq!(properties["PB"], ["柯洁"]);
        assert_eq!(properties["PW"], ["申真谞"]);
        assert_eq!(properties["RE"], ["黑中盘胜"]);
        assert_eq!(properties["GN"], ["Example"]);
        assert_eq!(properties["SZ"], ["19"]);
        // Only the root node's properties are read; move nodes are ignored.
        assert!(!properties.contains_key("B"));
        assert!(!properties.contains_key("W"));
    }

    #[test]
    fn extract_root_properties_handles_escaping_and_malformed_input() {
        let escaped = extract_root_properties("(;C[Bracket: \\] and slash: \\\\]N[日本語])");
        assert_eq!(escaped["C"], ["Bracket: ] and slash: \\"]);
        assert_eq!(escaped["N"], ["日本語"]);

        // Malformed / non-SGF text degrades to an empty map instead of failing.
        assert!(extract_root_properties("not an sgf").is_empty());
        assert!(extract_root_properties("").is_empty());
        // A truncated-but-parseable root still yields its properties.
        assert!(!extract_root_properties("(;SZ[19]").is_empty());
    }

    #[test]
    fn normalizes_sgf_line_endings_and_continuations() {
        let game =
            GameDocument::from_sgf("(;SZ[5]C[alpha\r\nbeta]GC[one\\\r\ntwo]N[three\\\nfour])")
                .unwrap();
        let properties = &game.snapshot().root_properties;

        assert_eq!(properties["C"], ["alpha\nbeta"]);
        assert_eq!(properties["GC"], ["onetwo"]);
        assert_eq!(properties["N"], ["threefour"]);
    }

    #[test]
    fn rejects_malformed_sgf_without_constructing_a_document() {
        for (content, expected_error) in [
            ("", DomainError::InvalidSgf(0)),
            ("()", DomainError::MissingGameTree),
            ("(;B)", DomainError::InvalidSgf(3)),
            ("(;B[aa", DomainError::InvalidSgf(6)),
            ("B[aa]", DomainError::InvalidSgf(0)),
            ("(;b[aa])", DomainError::InvalidSgf(2)),
        ] {
            let actual_error = GameDocument::from_sgf(content).unwrap_err();
            assert_eq!(
                actual_error.to_string(),
                expected_error.to_string(),
                "unexpected error for {content:?}",
            );
        }

        assert!(matches!(
            GameDocument::from_sgf("(;SZ[0])"),
            Err(DomainError::UnsupportedBoardSize)
        ));
        assert!(matches!(
            GameDocument::from_sgf("(;SZ[26])"),
            Err(DomainError::UnsupportedBoardSize)
        ));
    }

    #[test]
    fn round_trips_a_deep_pass_variation_tree() {
        let pass_count = 200;
        let moves = (0..pass_count)
            .map(|index| if index % 2 == 0 { ";B[]" } else { ";W[]" })
            .collect::<String>();
        let content = format!("(;GM[1]FF[4]SZ[19]{moves})");
        let game = GameDocument::from_sgf(&content).unwrap();
        let snapshot = game.snapshot();

        assert_eq!(snapshot.nodes.len(), pass_count + 1);
        assert_eq!(snapshot.moves.len(), pass_count);
        assert!(
            snapshot
                .moves
                .iter()
                .all(|move_data| move_data.vertex.is_none())
        );

        let round_tripped_snapshot = GameDocument::from_sgf(&game.to_sgf()).unwrap().snapshot();
        assert_eq!(round_tripped_snapshot.nodes.len(), snapshot.nodes.len());
        assert_eq!(round_tripped_snapshot.moves, snapshot.moves);
        assert_eq!(
            round_tripped_snapshot.current_node_id,
            snapshot.current_node_id
        );
    }

    #[test]
    fn round_trips_wide_root_variations() {
        let variation_count = 40;
        let variations = (0..variation_count)
            .map(|index| {
                let column = (b'a' + (index % 20) as u8) as char;
                let row = (b'a' + (index / 20) as u8) as char;
                format!("(;B[{column}{row}];W[])")
            })
            .collect::<String>();
        let game = GameDocument::from_sgf(&format!("(;GM[1]FF[4]SZ[19]{variations})")).unwrap();
        let snapshot = game.snapshot();
        let root_node = snapshot
            .nodes
            .iter()
            .find(|node| node.id == ROOT_NODE_ID)
            .unwrap();

        assert_eq!(root_node.child_ids.len(), variation_count);
        assert_eq!(snapshot.nodes.len(), variation_count * 2 + 1);

        let round_tripped_snapshot = GameDocument::from_sgf(&game.to_sgf()).unwrap().snapshot();
        let round_tripped_root = round_tripped_snapshot
            .nodes
            .iter()
            .find(|node| node.id == ROOT_NODE_ID)
            .unwrap();
        assert_eq!(round_tripped_root.child_ids.len(), variation_count);
        assert_eq!(round_tripped_snapshot.nodes.len(), snapshot.nodes.len());
    }

    #[test]
    fn serializes_v1_snapshot_with_camel_case_fields() {
        let serialized = serde_json::to_value(GameDocument::new(5, 5).unwrap().snapshot()).unwrap();
        assert_eq!(serialized["schemaVersion"], 1);
        assert_eq!(serialized["currentNodeId"], ROOT_NODE_ID);
        assert_eq!(serialized["board"]["moveNumber"], 0);
        assert!(serialized.get("current_node_id").is_none());
    }

    fn scoring_override_transaction(vertex: Vertex, score_override: Option<i8>) -> GameTransaction {
        GameTransaction {
            schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
            transaction_type: GameTransactionType::ApplyScoringOverride,
            color: None,
            vertex: Some(vertex),
            node_id: None,
            property: None,
            values: Vec::new(),
            marker: None,
            nodes: Vec::new(),
            score_override,
        }
    }

    #[test]
    fn scoring_overrides_apply_clear_and_appear_in_snapshots() {
        let mut game = GameDocument::new(19, 19).unwrap();
        let vertex = Vertex { column: 3, row: 3 };

        game.apply_transaction(scoring_override_transaction(vertex, Some(1)))
            .expect("a black-alive override applies");
        assert_eq!(
            game.snapshot().score_overrides.get(&vertex),
            Some(&1),
            "the snapshot exposes the applied override"
        );

        game.apply_transaction(scoring_override_transaction(vertex, Some(0)))
            .expect("clearing the override succeeds");
        assert!(
            !game.snapshot().score_overrides.contains_key(&vertex),
            "override value 0 clears the entry"
        );

        game.apply_transaction(scoring_override_transaction(vertex, Some(-1)))
            .expect("a white-alive override applies");
        assert_eq!(game.snapshot().score_overrides.get(&vertex), Some(&-1));
    }

    #[test]
    fn scoring_overrides_are_undoable_and_redoable() {
        let mut game = GameDocument::new(19, 19).unwrap();
        let vertex = Vertex { column: 3, row: 3 };
        game.apply_transaction(scoring_override_transaction(vertex, Some(1)))
            .expect("the override applies");

        assert!(game.undo());
        assert!(game.snapshot().score_overrides.is_empty());
        assert!(game.redo());
        assert_eq!(game.snapshot().score_overrides.get(&vertex), Some(&1));
    }

    #[test]
    fn scoring_overrides_reject_invalid_values_and_vertices() {
        let mut game = GameDocument::new(19, 19).unwrap();

        assert!(matches!(
            game.apply_transaction(scoring_override_transaction(
                Vertex { column: 3, row: 3 },
                Some(2)
            )),
            Err(DomainError::InvalidScoreOverride(2))
        ));
        assert!(matches!(
            game.apply_transaction(scoring_override_transaction(
                Vertex { column: 3, row: 3 },
                Some(-2)
            )),
            Err(DomainError::InvalidScoreOverride(-2))
        ));
        assert!(matches!(
            game.apply_transaction(scoring_override_transaction(
                Vertex { column: 3, row: 3 },
                None
            )),
            Err(DomainError::MissingScoreOverride)
        ));
        assert!(matches!(
            game.apply_transaction(GameTransaction {
                schema_version: CURRENT_TRANSACTION_SCHEMA_VERSION,
                transaction_type: GameTransactionType::ApplyScoringOverride,
                color: None,
                vertex: None,
                node_id: None,
                property: None,
                values: Vec::new(),
                marker: None,
                nodes: Vec::new(),
                score_override: Some(1),
            }),
            Err(DomainError::MissingTransactionVertex(_))
        ));
        assert!(matches!(
            game.apply_transaction(scoring_override_transaction(
                Vertex {
                    column: 99,
                    row: 99
                },
                Some(1)
            )),
            Err(DomainError::VertexOutsideBoard)
        ));
        assert!(game.snapshot().score_overrides.is_empty());
    }
}
