use std::rc::Rc;

use gpui::{
    App, Div, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, Styled, Window, div,
    px, rgb,
};
use sabaki_domain_core::{GameSnapshot, NodeId};

/// Horizontal distance between a node and its children in the tree layout.
const HORIZONTAL_SPACING_PX: f32 = 26.0;
/// Vertical distance between sibling branches in the tree layout.
const VERTICAL_SPACING_PX: f32 = 30.0;
/// Radius of a tree node dot.
const NODE_RADIUS_PX: f32 = 4.0;
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

/// Computes a two-dimensional layout for the game tree so it can be rendered
/// as an SVG-like node graph. The preferred child chain runs horizontally;
/// sibling branches hang vertically below their branching node. Returns the
/// bottom-most y coordinate used by the subtree.
fn layout_subtree(
    snapshot: &GameSnapshot,
    node_id: &str,
    x: f32,
    y: f32,
    parent_position: Option<(f32, f32)>,
    nodes: &mut Vec<LayoutedNode>,
) -> f32 {
    let is_current = snapshot.current_node_id == node_id;
    nodes.push(LayoutedNode {
        node_id: node_id.to_owned(),
        x,
        y,
        parent_position,
        is_current,
    });

    let children = children_of(snapshot, node_id);
    let preferred = preferred_child(snapshot, node_id);
    let mut bottom_y = y;

    let mut branch_y = y + VERTICAL_SPACING_PX;
    for child in &children {
        if Some(child) == preferred.as_ref() {
            continue;
        }
        let child_bottom = layout_subtree(
            snapshot,
            child,
            x + HORIZONTAL_SPACING_PX,
            branch_y,
            Some((x, y)),
            nodes,
        );
        bottom_y = bottom_y.max(child_bottom);
        branch_y = child_bottom + VERTICAL_SPACING_PX;
    }

    if let Some(preferred_id) = preferred {
        let preferred_bottom = layout_subtree(
            snapshot,
            &preferred_id,
            x + HORIZONTAL_SPACING_PX,
            y,
            Some((x, y)),
            nodes,
        );
        bottom_y = bottom_y.max(preferred_bottom);
    }

    bottom_y
}

/// Builds the tree layout for a snapshot. The layout is a pure function of the
/// snapshot so it can be unit-tested without a view.
pub fn build_variation_tree_layout(snapshot: &GameSnapshot) -> VariationTreeLayout {
    let mut nodes = Vec::new();
    layout_subtree(snapshot, &snapshot.root_node_id, 0.0, 0.0, None, &mut nodes);
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
pub fn render_variation_tree<F>(layout: &VariationTreeLayout, on_node_clicked: F) -> Div
where
    F: Fn(&NodeId, &mut Window, &mut App) + 'static,
{
    let line_color = 0x8a6d3b;
    let default_node_color = 0x3a2410;
    let current_node_color = 0xc0392b;
    let on_node_clicked = Rc::new(on_node_clicked);

    let mut children: Vec<Div> = Vec::new();

    for node in &layout.nodes {
        if let Some((parent_x, parent_y)) = node.parent_position {
            children.push(connector(
                (parent_x, parent_y),
                (node.x, node.y),
                line_color,
            ));
        }
    }

    for node in &layout.nodes {
        let handler = on_node_clicked.clone();
        let node_id = node.node_id.clone();
        let color = if node.is_current {
            current_node_color
        } else {
            default_node_color
        };
        children.push(
            div()
                .absolute()
                .left(px(node.x - NODE_RADIUS_PX))
                .top(px(node.y - NODE_RADIUS_PX))
                .size(px(NODE_RADIUS_PX * 2.0))
                .rounded_full()
                .border_1()
                .border_color(rgb(0x000000))
                .bg(rgb(color))
                .on_mouse_down(
                    MouseButton::Left,
                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        handler(&node_id, window, cx);
                    },
                ),
        );
    }

    div()
        .relative()
        .w(px(VARIATION_TREE_WIDTH_PX))
        .h(px(VARIATION_TREE_HEIGHT_PX))
        .children(children)
}

#[cfg(test)]
mod tests {
    use super::{build_variation_tree_layout, layout_subtree};
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
                    Some((node.x - super::HORIZONTAL_SPACING_PX, node.y)),
                    "linear children sit to the right of their parent"
                );
            }
        }

        assert!(layout.node("node-3").unwrap().is_current);
    }

    #[test]
    fn branching_game_places_siblings_vertically_below_the_branch() {
        let snapshot = snapshot_of("(;SZ[5];B[aa](;W[bb])(;W[cc]))");
        let layout = build_variation_tree_layout(&snapshot);

        let node_2 = layout.node("node-2").unwrap();
        let node_3 = layout.node("node-3").unwrap();

        assert_eq!(node_2.x, node_3.x, "siblings share the same column");
        assert!(
            node_3.y > node_2.y,
            "the second sibling hangs below the first"
        );
        assert_eq!(
            node_2.parent_position,
            Some((node_2.x - super::HORIZONTAL_SPACING_PX, node_2.y)),
            "the preferred child continues horizontally from its parent"
        );
        assert_eq!(
            node_3.parent_position,
            Some((
                node_3.x - super::HORIZONTAL_SPACING_PX,
                node_3.y - super::VERTICAL_SPACING_PX
            )),
            "a sibling branch hangs below its parent"
        );
    }

    #[test]
    fn current_node_is_marked_in_the_layout() {
        let snapshot = snapshot_of("(;SZ[5];B[aa];W[bb])");
        let layout = build_variation_tree_layout(&snapshot);
        assert_eq!(layout.node("node-2").unwrap().is_current, true);
        assert_eq!(layout.node("root").unwrap().is_current, false);
    }

    #[test]
    fn layout_subtree_returns_the_deepest_y() {
        let snapshot = snapshot_of("(;SZ[5];B[aa](;W[bb])(;W[cc]))");
        let mut nodes = Vec::new();
        let bottom = layout_subtree(&snapshot, "root", 0.0, 0.0, None, &mut nodes);
        assert!(bottom >= nodes.iter().map(|node| node.y).fold(0.0_f32, f32::max));
    }
}
