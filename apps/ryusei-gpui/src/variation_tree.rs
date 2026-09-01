use std::{collections::BTreeSet, rc::Rc};

use gpui::{
    App, Div, FontWeight, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Stateful,
    Styled, Window, div, px, rgb,
};
use ryusei_domain_core::{Color, GameSnapshot, NodeId, NodeSnapshot};
use ryusei_host::MoveQuality;

use crate::theme::UiPalette;

/// KaTrain-style GameGraph grid spacing:
/// Moves progress horizontally from Left to Right along the X-axis.
/// Alternate variations diverge downwards along the Y-axis.
pub const HORIZONTAL_SPACING_PX: f32 = 34.0;
pub const VERTICAL_SPACING_PX: f32 = 30.0;

/// Total base size of the rendered variation tree.
pub const VARIATION_TREE_WIDTH_PX: f32 = 260.0;
pub const VARIATION_TREE_HEIGHT_PX: f32 = 240.0;

/// A single node positioned by the KaTrain horizontal layout algorithm.
#[derive(Clone, Debug)]
pub struct LayoutedNode {
    pub node_id: NodeId,
    pub x: f32,
    pub y: f32,
    pub parent_position: Option<(f32, f32)>,
    pub is_current: bool,
    pub is_current_path: bool,
    pub kind: GameGraphNodeKind,
    pub color: GameGraphNodeColor,
    pub move_number: usize,
    pub stone_color: Option<Color>,
    pub ai_quality: Option<MoveQuality>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameGraphNodeKind {
    Circle,
    Square,
    Diamond,
    Bookmark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameGraphNodeColor {
    Neutral,
    Comment,
    BadMove,
    DoubtfulMove,
    InterestingMove,
    GoodMove,
}

#[derive(Clone, Debug, Default)]
pub struct VariationTreeLayout {
    pub nodes: Vec<LayoutedNode>,
}

impl VariationTreeLayout {
    #[cfg(test)]
    fn node(&self, node_id: &str) -> Option<&LayoutedNode> {
        self.nodes.iter().find(|node| node.node_id == node_id)
    }
}

/// Identifies the sequence of nodes leading from root to the current active node.
fn current_path(snapshot: &GameSnapshot) -> BTreeSet<NodeId> {
    let mut path = BTreeSet::new();
    let mut cursor = Some(snapshot.current_node_id.clone());
    while let Some(node_id) = cursor {
        let Some(node) = snapshot.nodes.iter().find(|node| node.id == node_id) else {
            break;
        };
        path.insert(node.id.clone());
        cursor = node.parent_id.clone();
    }
    path
}

fn node_kind(properties: &ryusei_domain_core::Properties) -> GameGraphNodeKind {
    if properties.contains_key("HO") {
        GameGraphNodeKind::Bookmark
    } else if properties.contains_key("B") || properties.contains_key("W") {
        let is_pass = ["B", "W"].into_iter().any(|property| {
            properties
                .get(property)
                .and_then(|values| values.first())
                .is_some_and(String::is_empty)
        });
        if is_pass {
            GameGraphNodeKind::Square
        } else {
            GameGraphNodeKind::Circle
        }
    } else {
        GameGraphNodeKind::Diamond
    }
}

fn node_color(properties: &ryusei_domain_core::Properties) -> GameGraphNodeColor {
    [
        ("BM", GameGraphNodeColor::BadMove),
        ("DO", GameGraphNodeColor::DoubtfulMove),
        ("IT", GameGraphNodeColor::InterestingMove),
        ("TE", GameGraphNodeColor::GoodMove),
    ]
    .into_iter()
    .find_map(|(property, color)| properties.contains_key(property).then_some(color))
    .unwrap_or_else(|| {
        if properties
            .keys()
            .any(|property| matches!(property.as_str(), "C" | "N" | "GC"))
        {
            GameGraphNodeColor::Comment
        } else {
            GameGraphNodeColor::Neutral
        }
    })
}

/// Calculates KaTrain AI move quality from explicit SGF marks or engine winrate/score drop.
fn compute_node_quality(node: &NodeSnapshot, parent: Option<&NodeSnapshot>) -> Option<MoveQuality> {
    let properties = &node.properties;

    // 1. Explicit SGF move quality marks
    if properties.contains_key("TE") {
        return Some(MoveQuality::Best);
    }
    if properties.contains_key("IT") {
        return Some(MoveQuality::Good);
    }
    if properties.contains_key("DO") {
        return Some(MoveQuality::Mistake);
    }
    if properties.contains_key("BM") {
        return Some(MoveQuality::Blunder);
    }

    // 2. Winrate & Score Lead evaluation (SBKV / SBKS) relative to previous state
    let curr_wr = properties
        .get("SBKV")
        .and_then(|v| v.first())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|s| (s / 100.0).clamp(0.0, 1.0));
    let curr_score = properties
        .get("SBKS")
        .and_then(|v| v.first())
        .and_then(|s| s.parse::<f64>().ok());

    if let (Some(curr_wr), Some(parent_node)) = (curr_wr, parent) {
        let prev_wr = parent_node
            .properties
            .get("SBKV")
            .and_then(|v| v.first())
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| (s / 100.0).clamp(0.0, 1.0))
            .unwrap_or(0.50);
        let prev_score = parent_node
            .properties
            .get("SBKS")
            .and_then(|v| v.first())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        let is_black = properties.contains_key("B");
        let (wr_drop, score_drop) = if is_black {
            (
                prev_wr - curr_wr,
                prev_score - curr_score.unwrap_or(prev_score),
            )
        } else {
            (
                curr_wr - prev_wr,
                curr_score.unwrap_or(prev_score) - prev_score,
            )
        };

        return Some(MoveQuality::classify(score_drop, wr_drop));
    }

    None
}

/// Recursively lays out the game tree from left (depth) to right, with alternate
/// branches placed on subsequent rows downwards.
fn layout_subtree(
    snapshot: &GameSnapshot,
    path: &BTreeSet<NodeId>,
    node_id: &str,
    depth: usize,
    row: usize,
    parent_position: Option<(f32, f32)>,
    nodes: &mut Vec<LayoutedNode>,
) -> usize {
    let x = depth as f32 * HORIZONTAL_SPACING_PX;
    let y = row as f32 * VERTICAL_SPACING_PX;
    let is_current = snapshot.current_node_id == node_id;
    let node_obj = snapshot
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .expect("layout nodes come from the snapshot");
    let properties = &node_obj.properties;

    let parent_node = node_obj
        .parent_id
        .as_deref()
        .and_then(|pid| snapshot.nodes.iter().find(|n| n.id == pid));

    let stone_color = if properties.contains_key("B") {
        Some(Color::Black)
    } else if properties.contains_key("W") {
        Some(Color::White)
    } else {
        None
    };

    let ai_quality = compute_node_quality(node_obj, parent_node);

    nodes.push(LayoutedNode {
        node_id: node_id.to_owned(),
        x,
        y,
        parent_position,
        is_current,
        is_current_path: path.contains(node_id),
        kind: node_kind(properties),
        color: node_color(properties),
        move_number: depth,
        stone_color,
        ai_quality,
    });

    let preferred = preferred_child(snapshot, node_id);
    let mut children = children_of(snapshot, node_id);
    if let Some(preferred) = preferred.as_ref() {
        children.retain(|child| child != preferred);
        children.insert(0, preferred.clone());
    }

    // The preferred branch continues horizontally on the same row.
    // Subsequent variation branches consume free rows below.
    let mut next_row = row + 1;
    for (index, child) in children.iter().enumerate() {
        let child_row = if index == 0 { row } else { next_row };
        let consumed_to = layout_subtree(
            snapshot,
            path,
            child,
            depth + 1,
            child_row,
            Some((x, y)),
            nodes,
        );
        next_row = next_row.max(consumed_to);
    }

    next_row
}

/// Builds the KaTrain-style horizontal variation tree layout for a game snapshot.
pub fn build_variation_tree_layout(snapshot: &GameSnapshot) -> VariationTreeLayout {
    let mut nodes = Vec::new();
    let path = current_path(snapshot);
    layout_subtree(
        snapshot,
        &path,
        &snapshot.root_node_id,
        0,
        0,
        None,
        &mut nodes,
    );
    VariationTreeLayout { nodes }
}

fn children_of(snapshot: &GameSnapshot, node_id: &str) -> Vec<NodeId> {
    snapshot
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .map(|node| node.child_ids.clone())
        .unwrap_or_default()
}

fn preferred_child(snapshot: &GameSnapshot, node_id: &str) -> Option<NodeId> {
    if let Some(preferred) = snapshot.preferred_child_by_node.get(node_id) {
        return Some(preferred.clone());
    }
    children_of(snapshot, node_id).into_iter().next()
}

/// Draws KaTrain-style step connectors between parent and child nodes flowing left-to-right.
fn connector(from: (f32, f32), to: (f32, f32), color: u32) -> Vec<Div> {
    let is_horizontal = (from.1 - to.1).abs() < 0.001;
    if is_horizontal {
        vec![
            div()
                .absolute()
                .left(px(from.0.min(to.0)))
                .top(px(from.1 - 0.75))
                .w(px((to.0 - from.0).abs()))
                .h(px(1.5))
                .bg(rgb(color)),
        ]
    } else {
        // Step branch connector: horizontal out -> vertical drop -> horizontal in
        let branch_x = from.0 + (to.0 - from.0).max(12.0) * 0.45;
        vec![
            div()
                .absolute()
                .left(px(from.0))
                .top(px(from.1 - 0.75))
                .w(px(branch_x - from.0))
                .h(px(1.5))
                .bg(rgb(color)),
            div()
                .absolute()
                .left(px(branch_x - 0.75))
                .top(px(from.1.min(to.1)))
                .w(px(1.5))
                .h(px((to.1 - from.1).abs()))
                .bg(rgb(color)),
            div()
                .absolute()
                .left(px(branch_x))
                .top(px(to.1 - 0.75))
                .w(px(to.0 - branch_x))
                .h(px(1.5))
                .bg(rgb(color)),
        ]
    }
}

/// Renders the KaTrain-style horizontal variation tree graph.
/// Each node shows its move number (手数) and KaTrain AI move quality colored border/ring.
pub fn render_variation_tree<F, G>(
    layout: &VariationTreeLayout,
    grid_size: f32,
    _node_size: f32,
    palette: UiPalette,
    on_node_clicked: F,
    on_node_context_requested: G,
) -> Div
where
    F: Fn(&NodeId, &mut Window, &mut App) + 'static,
    G: Fn(&NodeId, &mut Window, &mut App) + 'static,
{
    let line_color = 0x3f3f46; // Dark slate connector line
    let active_line_color = palette.accent;
    let grid_size = grid_size.max(14.0);
    let scale_x = (grid_size / HORIZONTAL_SPACING_PX).clamp(0.8, 1.4);
    let scale_y = (grid_size / VERTICAL_SPACING_PX).clamp(0.8, 1.4);
    let on_node_clicked = Rc::new(on_node_clicked);
    let on_node_context_requested = Rc::new(on_node_context_requested);

    let mut children: Vec<Stateful<Div>> = Vec::new();
    let padding = 20.0_f32;

    // Render tree branch connectors
    for (edge_index, node) in layout.nodes.iter().enumerate() {
        if let Some((parent_x, parent_y)) = node.parent_position {
            let color = if node.is_current_path {
                active_line_color
            } else {
                line_color
            };
            let segments = connector(
                (parent_x * scale_x + padding, parent_y * scale_y + padding),
                (node.x * scale_x + padding, node.y * scale_y + padding),
                color,
            );
            for (seg_idx, segment) in segments.into_iter().enumerate() {
                children.push(segment.id(("game-graph-edge", edge_index * 10 + seg_idx)));
            }
        }
    }

    // Render KaTrain circular nodes with move numbers & AI evaluation quality colors
    for (node_index, node) in layout.nodes.iter().enumerate() {
        let handler = on_node_clicked.clone();
        let context_handler = on_node_context_requested.clone();
        let node_id = node.node_id.clone();
        let context_node_id = node.node_id.clone();

        // Determine AI evaluation quality color (KaTrain color coding)
        let ai_quality_color = if let Some(quality) = node.ai_quality {
            quality.color_u32()
        } else {
            match node.color {
                GameGraphNodeColor::Neutral => 0x52525b,
                GameGraphNodeColor::Comment => 0x71717a,
                GameGraphNodeColor::BadMove => 0xef4444, // Blunder (Red)
                GameGraphNodeColor::DoubtfulMove => 0xf43f5e, // Mistake (Rose)
                GameGraphNodeColor::InterestingMove => 0x0ea5e9, // Good (Blue)
                GameGraphNodeColor::GoodMove => 0x10b981, // Best (Green)
            }
        };

        // Determine stone body & text color
        let (bg_color, text_color) = match node.stone_color {
            Some(Color::Black) => (0x18181b, 0xf4f4f5),
            Some(Color::White) => (0xf4f4f5, 0x18181b),
            None => (0x27272a, 0xa1a1aa), // Root or non-move node
        };

        let move_label = match node.kind {
            GameGraphNodeKind::Bookmark => "★".to_owned(),
            GameGraphNodeKind::Square => "P".to_owned(),
            _ => {
                if node.move_number == 0 {
                    "0".to_owned()
                } else {
                    node.move_number.to_string()
                }
            }
        };

        let visual_node_size = if node.is_current { 22.0 } else { 19.0 };
        let hitbox_size = (HORIZONTAL_SPACING_PX * scale_x).max(24.0);
        let x = node.x * scale_x + padding;
        let y = node.y * scale_y + padding;

        children.push(
            div()
                .id(("game-graph-node", node_index))
                .debug_selector(move || format!("game-graph-node-{node_index}"))
                .absolute()
                .left(px(x - hitbox_size / 2.0))
                .top(px(y - hitbox_size / 2.0))
                .size(px(hitbox_size))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .hover(|style| style.rounded_full().bg(rgb(palette.button_active)))
                .child(
                    div()
                        .size(px(visual_node_size))
                        .rounded_full()
                        .border_2()
                        .border_color(rgb(if node.is_current {
                            palette.accent // Active ring follows theme accent
                        } else {
                            ai_quality_color // KaTrain move quality border
                        }))
                        .bg(rgb(bg_color))
                        .shadow_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(text_color))
                        .child(move_label),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        handler(&node_id, window, cx);
                    },
                )
                .on_mouse_down(
                    MouseButton::Right,
                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        context_handler(&context_node_id, window, cx);
                    },
                ),
        );
    }

    let max_x = layout
        .nodes
        .iter()
        .map(|node| node.x)
        .fold(0.0_f32, f32::max)
        * scale_x;
    let max_y = layout
        .nodes
        .iter()
        .map(|node| node.y)
        .fold(0.0_f32, f32::max)
        * scale_y;

    div()
        .relative()
        .w(px(
            VARIATION_TREE_WIDTH_PX.max(max_x + grid_size + padding * 2.0)
        ))
        .h(px(
            VARIATION_TREE_HEIGHT_PX.max(max_y + grid_size + padding * 2.0)
        ))
        .children(children)
}

#[cfg(test)]
mod tests {
    use super::{build_variation_tree_layout, current_path, layout_subtree};
    use ryusei_domain_core::{GameDocument, GameSnapshot};

    fn snapshot_of(sgf: &str) -> GameSnapshot {
        GameDocument::from_sgf(sgf).unwrap().snapshot()
    }

    #[test]
    fn linear_game_layouts_nodes_horizontally_along_x_axis() {
        let snapshot = snapshot_of("(;SZ[5];B[aa];W[bb];B[cc])");
        let layout = build_variation_tree_layout(&snapshot);

        assert_eq!(layout.nodes.len(), 4, "root + 3 moves");
        let root = layout.node("root").unwrap();
        assert_eq!(root.x, 0.0);
        assert_eq!(root.y, 0.0);
        assert_eq!(root.parent_position, None);

        for node in &layout.nodes {
            if node.node_id != "root" {
                assert_eq!(
                    node.parent_position,
                    Some((node.x - super::HORIZONTAL_SPACING_PX, node.y)),
                    "linear children stay in their row and advance horizontally along x-axis"
                );
            }
        }

        assert!(layout.node("node-3").unwrap().is_current);
        assert_eq!(layout.node("node-1").unwrap().move_number, 1);
        assert_eq!(layout.node("node-2").unwrap().move_number, 2);
        assert_eq!(layout.node("node-3").unwrap().move_number, 3);
    }

    #[test]
    fn branching_game_places_siblings_in_collision_free_rows() {
        let snapshot = snapshot_of("(;SZ[5];B[aa](;W[bb])(;W[cc]))");
        let layout = build_variation_tree_layout(&snapshot);

        let node_2 = layout.node("node-2").unwrap();
        let node_3 = layout.node("node-3").unwrap();

        assert_eq!(
            node_2.x, node_3.x,
            "siblings share the same depth along x-axis"
        );
        assert!(
            node_3.y > node_2.y,
            "alternate variation gets a lower row along y-axis"
        );
        assert_eq!(
            node_2.parent_position,
            Some((super::HORIZONTAL_SPACING_PX, 0.0))
        );
        assert_eq!(
            node_3.parent_position,
            Some((super::HORIZONTAL_SPACING_PX, 0.0))
        );
    }

    #[test]
    fn current_node_is_marked_in_the_layout() {
        let snapshot = snapshot_of("(;SZ[5];B[aa];W[bb])");
        let layout = build_variation_tree_layout(&snapshot);
        assert!(layout.node("node-2").unwrap().is_current);
        assert!(!layout.node("root").unwrap().is_current);
    }

    #[test]
    fn graph_metadata_marks_path_node_move_number_and_ai_quality() {
        let snapshot = snapshot_of("(;SZ[5];B[]HO[1]TE[1](;W[bb]C[note])(;W[cc]BM[1]))");
        let layout = build_variation_tree_layout(&snapshot);
        let current = layout.node("node-2").unwrap();
        assert!(current.is_current);
        assert!(current.is_current_path);
        assert_eq!(current.kind, super::GameGraphNodeKind::Circle);
        assert_eq!(current.color, super::GameGraphNodeColor::Comment);
        assert_eq!(
            layout.node("node-3").unwrap().color,
            super::GameGraphNodeColor::BadMove
        );
        let bookmark = layout.node("node-1").unwrap();
        assert_eq!(bookmark.kind, super::GameGraphNodeKind::Bookmark);
        assert_eq!(bookmark.color, super::GameGraphNodeColor::GoodMove);
        assert_eq!(bookmark.ai_quality, Some(ryusei_host::MoveQuality::Best));
    }

    #[test]
    fn layout_subtree_returns_the_next_free_row() {
        let snapshot = snapshot_of("(;SZ[5];B[aa](;W[bb];B[cc])(;W[dd];B[ee]))");
        let mut nodes = Vec::new();
        let path = current_path(&snapshot);
        let next_row = layout_subtree(&snapshot, &path, "root", 0, 0, None, &mut nodes);
        assert!(next_row >= 2, "both branch rows are reserved");
        let occupied = nodes
            .iter()
            .map(|node| {
                (
                    (node.x / super::HORIZONTAL_SPACING_PX) as usize,
                    (node.y / super::VERTICAL_SPACING_PX) as usize,
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(occupied.len(), nodes.len(), "matrix cells never collide");
    }
}
