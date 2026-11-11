//! A deliberately small docking tree.
//!
//! egui's own panels are tied to the edges of the window; the point of this
//! module is the opposite — every region can be split, closed, and renamed,
//! like a Blender area. The tree is tiny enough that recursion is the clearest
//! implementation: a split divides its rect, recurses into both halves, and
//! places a draggable six-pixel handle on the seam.
//!
//! Structural edits (split, close, change kind) are collected while a frame is
//! being drawn and applied afterwards. Mutating the tree in the middle of the
//! recursion would invalidate the paths of every sibling.

use eframe::egui::{
    self, Align, CornerRadius, CursorIcon, Layout as UiLayout, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, pos2, vec2,
};

use crate::theme;

/// Height of a panel's little title bar.
pub const HEADER_HEIGHT: f32 = 30.0;
/// Width of a split's drag handle, in points.
const DIVIDER_WIDTH: f32 = 6.0;
/// Inner padding of a panel body.
const BODY_PADDING: f32 = 12.0;

/// The five things a dock leaf can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Editor,
    Palette,
    Variables,
    Plot,
    History,
}

impl PanelKind {
    pub const ALL: [Self; 5] = [
        Self::Editor,
        Self::Palette,
        Self::Variables,
        Self::Plot,
        Self::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::Palette => "Palette",
            Self::Variables => "Variables",
            Self::Plot => "Plot",
            Self::History => "History",
        }
    }
}

/// Which way a split divides its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Side by side: the first child is on the left.
    Horizontal,
    /// Stacked: the first child is on top.
    Vertical,
}

impl Axis {
    fn splitters(self) -> (f32, f32) {
        match self {
            Self::Horizontal => (1.0, 0.0),
            Self::Vertical => (0.0, 1.0),
        }
    }
}

/// The layout tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Leaf(PanelKind),
    Split {
        axis: Axis,
        /// Share of the space given to `a`, clamped to `0.1..=0.9` while
        /// dragging.
        fraction: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Default for Layout {
    fn default() -> Self {
        Workspace::default().layout()
    }
}

/// Preset arrangements offered by the menu bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Workspace {
    #[default]
    Editing,
    Solving,
    Plotting,
}

impl Workspace {
    pub const ALL: [Self; 3] = [Self::Editing, Self::Solving, Self::Plotting];

    pub fn label(self) -> &'static str {
        match self {
            Self::Editing => "Editing",
            Self::Solving => "Solving",
            Self::Plotting => "Plotting",
        }
    }

    /// All five panels stay reachable in every workspace; the thing that
    /// changes is which one gets the large area.
    pub fn layout(self) -> Layout {
        match self {
            Self::Editing => row(
                0.18,
                leaf(PanelKind::Palette),
                row(
                    0.80,
                    leaf(PanelKind::Editor),
                    col(
                        0.52,
                        leaf(PanelKind::Variables),
                        col(0.54, leaf(PanelKind::Plot), leaf(PanelKind::History)),
                    ),
                ),
            ),
            Self::Solving => col(
                0.62,
                row(0.74, leaf(PanelKind::Editor), leaf(PanelKind::Variables)),
                row(
                    0.66,
                    leaf(PanelKind::History),
                    row(0.5, leaf(PanelKind::Palette), leaf(PanelKind::Plot)),
                ),
            ),
            Self::Plotting => row(
                0.17,
                leaf(PanelKind::Palette),
                col(
                    0.52,
                    leaf(PanelKind::Editor),
                    row(
                        0.72,
                        leaf(PanelKind::Plot),
                        col(0.58, leaf(PanelKind::Variables), leaf(PanelKind::History)),
                    ),
                ),
            ),
        }
    }
}

/// The dock's persistent state.
pub struct Dock {
    layout: Layout,
    workspace: Workspace,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            layout: Workspace::default().layout(),
            workspace: Workspace::default(),
        }
    }
}

impl Dock {
    pub fn workspace(&self) -> Workspace {
        self.workspace
    }

    /// Replace the tree with the preset for `workspace`.
    pub fn select_workspace(&mut self, workspace: Workspace) {
        self.workspace = workspace;
        self.layout = workspace.layout();
    }

    /// Re-apply the current workspace's preset.
    pub fn reset(&mut self) {
        self.layout = self.workspace.layout();
    }

    /// Take the tree out so the caller can borrow the app mutably while the
    /// panels render. Put it back with [`Dock::restore`].
    pub fn take_layout(&mut self) -> Layout {
        std::mem::take(&mut self.layout)
    }

    pub fn restore(&mut self, layout: Layout) {
        self.layout = layout;
    }
}

fn leaf(kind: PanelKind) -> Layout {
    Layout::Leaf(kind)
}

fn row(fraction: f32, a: Layout, b: Layout) -> Layout {
    split(Axis::Horizontal, fraction, a, b)
}

fn col(fraction: f32, a: Layout, b: Layout) -> Layout {
    split(Axis::Vertical, fraction, a, b)
}

fn split(axis: Axis, fraction: f32, a: Layout, b: Layout) -> Layout {
    Layout::Split {
        axis,
        fraction,
        a: Box::new(a),
        b: Box::new(b),
    }
}

/// One structural edit requested by a header button.
enum Action {
    SetKind {
        path: Vec<usize>,
        kind: PanelKind,
    },
    Split {
        path: Vec<usize>,
        axis: Axis,
        kind: PanelKind,
    },
    Close {
        path: Vec<usize>,
    },
}

impl Layout {
    /// Draw the whole tree into `rect`.
    ///
    /// `panel` is called for every leaf with a UI clipped to the leaf's body.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, mut panel: impl FnMut(&mut Ui, PanelKind)) {
        if rect.width() < 2.0 || rect.height() < 2.0 {
            return;
        }
        let mut actions = Vec::new();
        self.show_node(ui, rect, &mut Vec::new(), &mut panel, &mut actions);
        for action in actions {
            self.apply(action);
        }
    }

    fn show_node(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        path: &mut Vec<usize>,
        panel: &mut impl FnMut(&mut Ui, PanelKind),
        actions: &mut Vec<Action>,
    ) {
        match self {
            Self::Leaf(kind) => show_leaf(ui, rect, *kind, path, panel, actions),
            Self::Split {
                axis,
                fraction,
                a,
                b,
            } => {
                let axis = *axis;
                let mut share = *fraction;
                let (a_rect, b_rect, divider) = divide(rect, axis, share);
                if a_rect.width() > 1.0 && a_rect.height() > 1.0 {
                    path.push(0);
                    a.show_node(ui, a_rect, path, panel, actions);
                    path.pop();
                }
                if b_rect.width() > 1.0 && b_rect.height() > 1.0 {
                    path.push(1);
                    b.show_node(ui, b_rect, path, panel, actions);
                    path.pop();
                }

                let id = ui.make_persistent_id(("canlab-dock-divider", path.clone()));
                let response = ui.interact(divider, id, Sense::drag());
                let cursor = match axis {
                    Axis::Horizontal => CursorIcon::ResizeHorizontal,
                    Axis::Vertical => CursorIcon::ResizeVertical,
                };
                let response = response.on_hover_cursor(cursor);
                if response.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                    let (dx, dy) = axis.splitters();
                    let delta = response.drag_delta();
                    let span = if axis == Axis::Horizontal {
                        rect.width()
                    } else {
                        rect.height()
                    };
                    if span > 1.0 {
                        share = (share + (delta.x * dx + delta.y * dy) / span).clamp(0.1, 0.9);
                    }
                }
                if response.hovered() || response.dragged() {
                    let line = match axis {
                        Axis::Horizontal => Rect::from_center_size(
                            divider.center(),
                            egui::vec2(2.0, divider.height()),
                        ),
                        Axis::Vertical => Rect::from_center_size(
                            divider.center(),
                            egui::vec2(divider.width(), 2.0),
                        ),
                    };
                    ui.painter()
                        .rect_filled(line, CornerRadius::same(1), theme::accent());
                }
                *fraction = share;
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::SetKind { path, kind } => {
                if let Some(Layout::Leaf(current)) = self.at_mut(&path) {
                    *current = kind;
                }
            }
            Action::Split { path, axis, kind } => {
                let Some(slot) = self.at_mut(&path) else {
                    return;
                };
                let old = std::mem::replace(slot, Layout::Leaf(kind));
                *slot = split(axis, 0.5, old, Layout::Leaf(kind));
            }
            Action::Close { path } => {
                self.close(&path);
            }
        }
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut Layout> {
        let mut node = self;
        for &index in path {
            node = match node {
                Layout::Split { a, b, .. } => {
                    if index == 0 {
                        &mut **a
                    } else {
                        &mut **b
                    }
                }
                Layout::Leaf(_) => return None,
            };
        }
        Some(node)
    }

    /// Remove the leaf at `path` and promote its sibling into the parent's
    /// slot. The last remaining panel cannot be closed.
    fn close(&mut self, path: &[usize]) -> bool {
        let Some((&branch, parent)) = path.split_last() else {
            return false;
        };
        if branch > 1 {
            return false;
        }
        let kept = match self.at_mut(parent) {
            Some(Layout::Split { a, b, .. }) => {
                let keep = if branch == 0 { b } else { a };
                std::mem::replace(&mut **keep, Layout::Leaf(PanelKind::Editor))
            }
            _ => return false,
        };
        if let Some(slot) = self.at_mut(parent) {
            *slot = kept;
            true
        } else {
            false
        }
    }
}

fn divide(rect: Rect, axis: Axis, fraction: f32) -> (Rect, Rect, Rect) {
    let half = DIVIDER_WIDTH * 0.5;
    match axis {
        Axis::Horizontal => {
            let x = rect.left() + rect.width() * fraction;
            (
                Rect::from_min_max(rect.min, pos2(x - half, rect.max.y)),
                Rect::from_min_max(pos2(x + half, rect.min.y), rect.max),
                Rect::from_center_size(
                    pos2(x, rect.center().y),
                    vec2(DIVIDER_WIDTH, rect.height()),
                ),
            )
        }
        Axis::Vertical => {
            let y = rect.top() + rect.height() * fraction;
            (
                Rect::from_min_max(rect.min, pos2(rect.max.x, y - half)),
                Rect::from_min_max(pos2(rect.min.x, y + half), rect.max),
                Rect::from_center_size(pos2(rect.center().x, y), vec2(rect.width(), DIVIDER_WIDTH)),
            )
        }
    }
}

fn show_leaf(
    ui: &mut Ui,
    rect: Rect,
    kind: PanelKind,
    path: &[usize],
    panel: &mut impl FnMut(&mut Ui, PanelKind),
    actions: &mut Vec<Action>,
) {
    let visuals = ui.visuals();
    let panel_fill = visuals.panel_fill;
    let header_fill = visuals.faint_bg_color;
    let border = Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);

    let painter = ui.painter();
    painter.rect_filled(rect.shrink(0.5), CornerRadius::same(5), panel_fill);
    painter.rect_stroke(
        rect.shrink(0.5),
        CornerRadius::same(5),
        border,
        StrokeKind::Inside,
    );

    let header_rect = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + HEADER_HEIGHT));
    painter.rect_filled(
        header_rect.shrink(1.0),
        CornerRadius {
            nw: 5,
            ne: 5,
            sw: 0,
            se: 0,
        },
        header_fill,
    );
    painter.line_segment(
        [header_rect.left_bottom(), header_rect.right_bottom()],
        border,
    );

    let mut header = ui.new_child(
        UiBuilder::new()
            .max_rect(header_rect.shrink2(vec2(9.0, 4.0)))
            .layout(UiLayout::left_to_right(Align::Center)),
    );
    header.set_clip_rect(header_rect);
    header.spacing_mut().item_spacing = vec2(5.0, 4.0);
    header.spacing_mut().button_padding = vec2(4.0, 2.0);
    header.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.0, theme::accent());
        ui.label(RichText::new(kind.label().to_uppercase()).small().strong());
        ui.with_layout(UiLayout::right_to_left(Align::Center), |ui| {
            if ui.small_button("×").on_hover_text("Close panel").clicked() {
                actions.push(Action::Close {
                    path: path.to_vec(),
                });
            }
            if ui
                .small_button("V")
                .on_hover_text("Split vertically")
                .clicked()
            {
                actions.push(Action::Split {
                    path: path.to_vec(),
                    axis: Axis::Vertical,
                    kind,
                });
            }
            if ui
                .small_button("H")
                .on_hover_text("Split horizontally")
                .clicked()
            {
                actions.push(Action::Split {
                    path: path.to_vec(),
                    axis: Axis::Horizontal,
                    kind,
                });
            }
            // Plain ASCII: the bundled font set is Ubuntu-Light plus egui's icon
            // font, and neither has the typographic ellipsis or the triangle this
            // button used to ask for. The headless frame test found both by
            // panicking on the missing glyph — which is the behaviour we want from
            // a test, and not from a UI that silently draws a box.
            let response = ui.menu_button("...", |ui| {
                ui.label(RichText::new("Panel").small().strong());
                ui.separator();
                for candidate in PanelKind::ALL {
                    if ui
                        .selectable_label(candidate == kind, candidate.label())
                        .clicked()
                    {
                        actions.push(Action::SetKind {
                            path: path.to_vec(),
                            kind: candidate,
                        });
                        ui.close();
                    }
                }
            });
            response.response.on_hover_text("Change panel");
        });
    });

    let body = Rect::from_min_max(
        pos2(
            rect.left() + BODY_PADDING,
            header_rect.bottom() + BODY_PADDING * 0.7,
        ),
        pos2(
            rect.right() - BODY_PADDING,
            rect.bottom() - BODY_PADDING * 0.7,
        ),
    );
    if body.width() > 16.0 && body.height() > 16.0 {
        let mut body_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(body)
                .layout(UiLayout::top_down(Align::Min)),
        );
        body_ui.set_clip_rect(body);
        body_ui.set_width(body.width());
        panel(&mut body_ui, kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(layout: &Layout, out: &mut Vec<PanelKind>) {
        match layout {
            Layout::Leaf(kind) => out.push(*kind),
            Layout::Split { a, b, .. } => {
                leaves(a, out);
                leaves(b, out);
            }
        }
    }

    #[test]
    fn every_workspace_shows_every_panel_once() {
        for workspace in Workspace::ALL {
            let mut kinds = Vec::new();
            leaves(&workspace.layout(), &mut kinds);
            assert_eq!(kinds.len(), PanelKind::ALL.len());
            for kind in PanelKind::ALL {
                assert!(kinds.contains(&kind), "{workspace:?} is missing {kind:?}");
            }
        }
    }

    #[test]
    fn splitting_a_leaf_makes_two_leaves() {
        let mut layout = Layout::Leaf(PanelKind::Editor);
        layout.apply(Action::Split {
            path: Vec::new(),
            axis: Axis::Horizontal,
            kind: PanelKind::Plot,
        });
        assert_eq!(
            layout,
            row(0.5, leaf(PanelKind::Editor), leaf(PanelKind::Plot))
        );
    }

    #[test]
    fn closing_a_leaf_promotes_its_sibling() {
        let mut layout = row(0.4, leaf(PanelKind::Editor), leaf(PanelKind::Plot));
        assert!(layout.close(&[0]));
        assert_eq!(layout, leaf(PanelKind::Plot));
    }

    #[test]
    fn closing_a_nested_leaf_keeps_the_rest_of_the_tree() {
        let mut layout = row(
            0.4,
            leaf(PanelKind::Palette),
            col(0.5, leaf(PanelKind::Editor), leaf(PanelKind::History)),
        );
        assert!(layout.close(&[1, 1]));
        assert_eq!(
            layout,
            row(0.4, leaf(PanelKind::Palette), leaf(PanelKind::Editor))
        );
    }

    #[test]
    fn the_last_panel_cannot_be_closed() {
        let mut layout = Layout::Leaf(PanelKind::Editor);
        assert!(!layout.close(&[]));
        assert_eq!(layout, leaf(PanelKind::Editor));
    }

    #[test]
    fn stale_paths_are_ignored() {
        let layout = row(0.5, leaf(PanelKind::Editor), leaf(PanelKind::Plot));
        let mut mutable = layout.clone();
        assert!(!mutable.close(&[7]));
        assert_eq!(mutable, layout);
    }
}
