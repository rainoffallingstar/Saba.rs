use std::{collections::BTreeSet, rc::Rc};

use gpui::{
    App, Div, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Stateful, Styled,
    Window, div, px, rgb,
};
use sabaki_domain_core::{GameSnapshot, NodeId};

use crate::theme::UiPalette;

/// Default GameGraph grid spacing when no persisted value exists.
const HORIZONTAL_SPACING_PX: f32 = 26.0;
const VERTICAL_SPACING_PX: f32 = 30.0;
/// Total size of the rendered variation tree.
pub const VARIATION_TREE_WIDTH_PX: f32 = 260.0;
pub const VARIATION_TREE_HEIGHT_PX: f32 = 240.0;

/// A single node positioned by the layout algorithm. Parent coordinates let
/// the renderer draw connectors between a node and its parent.
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

/// Computes the GameGraph matrix. Rows are move depth and the preferred child
/// remains in its parent's column; alternate variations consume collision-free
/// columns to the right. This mirrors Sabaki's matrix contract while keeping the
/// result as ordinary pixel coordinates for GPUI.
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

fn node_kind(properties: &sabaki_domain_core::Properties) -> GameGraphNodeKind {
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

fn node_color(properties: &sabaki_domain_core::Properties) -> GameGraphNodeColor {
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

fn layout_subtree(
    snapshot: &GameSnapshot,
    path: &BTreeSet<NodeId>,
    node_id: &str,
    column: usize,
    depth: usize,
    parent_position: Option<(f32, f32)>,
    nodes: &mut Vec<LayoutedNode>,
) -> usize {
    let x = column as f32 * HORIZONTAL_SPACING_PX;
    let y = depth as f32 * VERTICAL_SPACING_PX;
    let is_current = snapshot.current_node_id == node_id;
    let properties = snapshot
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .map(|node| &node.properties)
        .expect("layout nodes come from the snapshot");
    nodes.push(LayoutedNode {
        node_id: node_id.to_owned(),
        x,
        y,
        parent_position,
        is_current,
        is_current_path: path.contains(node_id),
        kind: node_kind(properties),
        color: node_color(properties),
    });

    let preferred = preferred_child(snapshot, node_id);
    let mut children = children_of(snapshot, node_id);
    if let Some(preferred) = preferred.as_ref() {
        children.retain(|child| child != preferred);
        children.insert(0, preferred.clone());
    }

    // The first/preferred branch inherits this column. Its occupied columns are
    // reserved before alternate subtrees get their own starts.
    let mut next_column = column + 1;
    for (index, child) in children.iter().enumerate() {
        let child_column = if index == 0 { column } else { next_column };
        let consumed_to = layout_subtree(
            snapshot,
            path,
            child,
            child_column,
            depth + 1,
            Some((x, y)),
            nodes,
        );
        next_column = next_column.max(consumed_to);
    }

    next_column
}

/// Builds the tree layout for a snapshot. The layout is a pure function of the
/// snapshot so it can be unit-tested without a view.
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

fn connector(from: (f32, f32), to: (f32, f32), color: u32) -> Div {
    let horizontal = (from.1 - to.1).abs() < 0.001;
    if horizontal {
        div()
            .absolute()
            .left(px(from.0.min(to.0)))
            .top(px(from.1 - 0.5))
            .w(px((to.0 - from.0).abs()))
            .h(px(1.0))
            .bg(rgb(color))
    } else {
        div()
            .absolute()
            .left(px(from.0 - 0.5))
            .top(px(from.1.min(to.1)))
            .w(px(1.0))
            .h(px((to.1 - from.1).abs()))
            .bg(rgb(color))
    }
}

/// Renders the variation tree as a flat node graph. Clicking a node invokes
/// `on_node_clicked` with that node's id.
pub fn render_variation_tree<F, G>(
    layout: &VariationTreeLayout,
    grid_size: f32,
    node_size: f32,
    palette: UiPalette,
    on_node_clicked: F,
    on_node_context_requested: G,
) -> Div
where
    F: Fn(&NodeId, &mut Window, &mut App) + 'static,
    G: Fn(&NodeId, &mut Window, &mut App) + 'static,
{
    let line_color = palette.accent;
    let current_node_color = palette.danger_text;
    let grid_size = grid_size.max(12.0);
    let node_size = node_size.max(3.0);
    let scale_x = grid_size / HORIZONTAL_SPACING_PX;
    let scale_y = grid_size / VERTICAL_SPACING_PX;
    let on_node_clicked = Rc::new(on_node_clicked);
    let on_node_context_requested = Rc::new(on_node_context_requested);

    let mut children: Vec<Stateful<Div>> = Vec::new();

    let padding = 16.0_f32;
    for (edge_index, node) in layout.nodes.iter().enumerate() {
        if let Some((parent_x, parent_y)) = node.parent_position {
            children.push(
                connector(
                    (parent_x * scale_x + padding, parent_y * scale_y + padding),
                    (node.x * scale_x + padding, node.y * scale_y + padding),
                    line_color,
                )
                .id(("game-graph-edge", edge_index)),
            );
        }
    }

    for (node_index, node) in layout.nodes.iter().enumerate() {
        let handler = on_node_clicked.clone();
        let context_handler = on_node_context_requested.clone();
        let node_id = node.node_id.clone();
        let context_node_id = node.node_id.clone();
        let color = if node.is_current {
            current_node_color
        } else {
            match node.color {
                GameGraphNodeColor::Neutral => palette.text,
                GameGraphNodeColor::Comment => palette.subtle,
                GameGraphNodeColor::BadMove => palette.danger_text,
                GameGraphNodeColor::DoubtfulMove => 0xc98a00,
                GameGraphNodeColor::InterestingMove => palette.accent,
                GameGraphNodeColor::GoodMove => palette.success,
            }
        };
        let (shape_scale, label) = match node.kind {
            GameGraphNodeKind::Circle => (1.0, ""),
            GameGraphNodeKind::Square => (1.0, "P"),
            GameGraphNodeKind::Diamond => (1.0, "+"),
            GameGraphNodeKind::Bookmark => (1.2, "H"),
        };
        let visual_node_size = (node_size * shape_scale * 2.0).clamp(10.0, 18.0);
        let hitbox_size = grid_size.max(22.0);
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
                .hover(|style| style.rounded_full().bg(rgb(palette.button)))
                .child(
                    div()
                        .size(px(visual_node_size))
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(if node.is_current || node.is_current_path {
                            palette.accent
                        } else {
                            palette.border
                        }))
                        .bg(rgb(color))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .child(label),
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
    use sabaki_domain_core::{GameDocument, GameSnapshot};

    fn snapshot_of(sgf: &str) -> GameSnapshot {
        GameDocument::from_sgf(sgf).unwrap().snapshot()
    }

    #[test]
    fn linear_game_layouts_nodes_along_the_preferred_chain() {
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
                    Some((node.x, node.y - super::VERTICAL_SPACING_PX)),
                    "linear children stay in their column and advance one row"
                );
            }
        }

        assert!(layout.node("node-3").unwrap().is_current);
    }

    #[test]
    fn branching_game_places_siblings_in_collision_free_columns() {
        let snapshot = snapshot_of("(;SZ[5];B[aa](;W[bb])(;W[cc]))");
        let layout = build_variation_tree_layout(&snapshot);

        let node_2 = layout.node("node-2").unwrap();
        let node_3 = layout.node("node-3").unwrap();

        assert_eq!(node_2.y, node_3.y, "siblings share the same depth row");
        assert!(
            node_3.x > node_2.x,
            "alternate variation gets a right column"
        );
        assert_eq!(node_2.parent_position, Some((0.0, 30.0)));
        assert_eq!(node_3.parent_position, Some((0.0, 30.0)));
    }

    #[test]
    fn current_node_is_marked_in_the_layout() {
        let snapshot = snapshot_of("(;SZ[5];B[aa];W[bb])");
        let layout = build_variation_tree_layout(&snapshot);
        assert!(layout.node("node-2").unwrap().is_current);
        assert!(!layout.node("root").unwrap().is_current);
    }

    #[test]
    fn graph_metadata_marks_path_node_shape_and_annotation_color() {
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
    }

    #[test]
    fn layout_subtree_returns_the_next_free_column() {
        let snapshot = snapshot_of("(;SZ[5];B[aa](;W[bb];B[cc])(;W[dd];B[ee]))");
        let mut nodes = Vec::new();
        let path = current_path(&snapshot);
        let next_column = layout_subtree(&snapshot, &path, "root", 0, 0, None, &mut nodes);
        assert!(next_column >= 2, "both branch columns are reserved");
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
