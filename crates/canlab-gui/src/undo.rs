//! Undo and redo for the document.
//!
//! The whole editor state is one [`MathField`], which is `Clone`, so undo is a
//! ring of snapshots rather than a command log. That is the right trade at this
//! size: a document is a few kilobytes, a hundred of them is nothing, and there is
//! no way for the undo path to *disagree* with the edit path — the state it
//! restores is a state the editor really was in.
//!
//! # Coalescing
//!
//! A snapshot per keystroke would make Ctrl+Z step back one character, which is
//! not what anyone means by undo. Changes closer together than [`COALESCE`] are
//! one step, so a burst of typing undoes as a burst. A *structural* change (a new
//! line, a palette insert, a paste) always starts a new step, because those are
//! the ones people want to take back one at a time.
//!
//! The snapshot is taken *before* the change is applied, so the first keystroke of
//! a burst is what lands in the ring: undoing a burst leaves the field as it was
//! before the burst began.

use std::time::{Duration, Instant};

use canlab_input::MathField;

/// How long a burst of typing lasts: changes closer together than this undo as one
/// step.
pub const COALESCE: Duration = Duration::from_millis(400);

/// How many steps are kept. A long document's snapshot is small, but the ring is
/// bounded anyway: an unbounded history of a document you have been editing for an
/// hour is a memory leak with good manners.
pub const DEPTH: usize = 100;

/// The undo ring.
pub struct Undo {
    past: Vec<MathField>,
    future: Vec<MathField>,
    /// When the current burst last saw a change.
    last: Option<Instant>,
}

impl Default for Undo {
    fn default() -> Self {
        Self::new()
    }
}

impl Undo {
    /// An empty history.
    pub fn new() -> Self {
        Undo {
            past: Vec::new(),
            future: Vec::new(),
            last: None,
        }
    }

    /// Record `field` before an edit.
    ///
    /// Call this *before* mutating the field. `structural` forces a new step; a
    /// plain keystroke joins the current burst when it is recent enough.
    pub fn checkpoint(&mut self, field: &MathField, structural: bool) {
        let now = Instant::now();
        let fresh = structural
            || self
                .last
                .is_none_or(|last| now.duration_since(last) > COALESCE);
        if fresh {
            self.past.push(field.clone());
            if self.past.len() > DEPTH {
                self.past.remove(0);
            }
            // A new edit invalidates the redo branch, as in every editor.
            self.future.clear();
        }
        self.last = Some(now);
    }

    /// End the current burst, so the next edit starts a new step.
    ///
    /// Called when focus leaves the editor or a command runs, so a pause and a
    /// resume are not silently fused into one undo step.
    pub fn break_burst(&mut self) {
        self.last = None;
    }

    /// Step back. Returns false when there is nothing to undo.
    pub fn undo(&mut self, field: &mut MathField) -> bool {
        let Some(previous) = self.past.pop() else {
            return false;
        };
        let current = std::mem::replace(field, previous);
        self.future.push(current);
        self.last = None;
        true
    }

    /// Step forward again. Returns false when there is nothing to redo.
    pub fn redo(&mut self, field: &mut MathField) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        let current = std::mem::replace(field, next);
        self.past.push(current);
        self.last = None;
        true
    }

    /// How many steps back are available.
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// How many steps forward are available.
    pub fn redo_depth(&self) -> usize {
        self.future.len()
    }

    /// True when Ctrl+Z would do something.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// True when Ctrl+Shift+Z would do something.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field with one more character in it each time it is touched.
    fn typed(field: &mut MathField, text: &str) {
        field.type_str(text);
    }

    #[test]
    fn an_edit_can_be_taken_back() {
        let mut field = MathField::new();
        typed(&mut field, "x");
        let mut undo = Undo::new();
        undo.checkpoint(&field, true);
        typed(&mut field, "+1");
        assert_eq!(field.to_latex(), "x+1");

        assert!(undo.undo(&mut field));
        assert_eq!(field.to_latex(), "x", "the state before the edit");
        assert!(undo.can_redo());
        assert!(undo.redo(&mut field));
        assert_eq!(field.to_latex(), "x+1");
    }

    #[test]
    fn nothing_to_undo_is_false_not_a_panic() {
        let mut field = MathField::new();
        let mut undo = Undo::new();
        assert!(!undo.undo(&mut field));
        assert!(!undo.redo(&mut field));
        assert_eq!(field.to_latex(), "");
    }

    #[test]
    fn a_burst_of_typing_is_one_step() {
        let mut field = MathField::new();
        let mut undo = Undo::new();
        // Three keystrokes inside one burst: one checkpoint before the first, and
        // the ring must not grow on the next two.
        undo.checkpoint(&field, false);
        typed(&mut field, "a");
        undo.checkpoint(&field, false);
        typed(&mut field, "b");
        undo.checkpoint(&field, false);
        typed(&mut field, "c");
        assert_eq!(field.to_latex(), "abc");
        assert_eq!(undo.undo_depth(), 1);

        assert!(undo.undo(&mut field));
        assert_eq!(field.to_latex(), "", "the whole burst came back off");
    }

    #[test]
    fn a_structural_change_is_always_its_own_step() {
        let mut field = MathField::new();
        let mut undo = Undo::new();
        undo.checkpoint(&field, false);
        typed(&mut field, "x");
        // Two structural changes in the same instant still count twice.
        undo.checkpoint(&field, true);
        field.new_line();
        undo.checkpoint(&field, true);
        typed(&mut field, "y");
        assert_eq!(undo.undo_depth(), 3);
        assert_eq!(field.line_count(), 2);

        assert!(undo.undo(&mut field));
        assert_eq!(field.line_count(), 2, "the line is still there");
        assert_eq!(field.cursor().path.len(), 0);

        assert!(undo.undo(&mut field));
        assert_eq!(field.line_count(), 1, "the second line went away");
        assert_eq!(field.to_latex(), "x");

        assert!(undo.undo(&mut field));
        assert_eq!(field.to_latex(), "", "and the first character too");
    }

    #[test]
    fn a_new_edit_drops_the_redo_branch() {
        let mut field = MathField::new();
        let mut undo = Undo::new();
        undo.checkpoint(&field, true);
        typed(&mut field, "x");
        assert!(undo.undo(&mut field));
        assert!(undo.can_redo());

        let before = field.clone();
        undo.checkpoint(&before, true);
        typed(&mut field, "y");
        assert!(!undo.can_redo(), "the old branch is unreachable now");
        assert!(!undo.redo(&mut field));
    }

    #[test]
    fn the_ring_is_bounded() {
        let mut field = MathField::new();
        let mut undo = Undo::new();
        for _ in 0..DEPTH + 20 {
            undo.checkpoint(&field, true);
            typed(&mut field, "x");
        }
        assert_eq!(undo.undo_depth(), DEPTH);
    }

    #[test]
    fn a_break_between_bursts_starts_a_new_step() {
        let mut field = MathField::new();
        let mut undo = Undo::new();
        undo.checkpoint(&field, false);
        typed(&mut field, "x");
        undo.break_burst();
        undo.checkpoint(&field, false);
        typed(&mut field, "y");
        assert_eq!(undo.undo_depth(), 2);
    }

    #[test]
    fn undoing_a_system_change_restores_the_whole_field() {
        // The thing undo is really for: taking back a structural change without
        // leaving the document half-edited.
        let mut field = MathField::new();
        field.type_str("x=1");
        let mut undo = Undo::new();
        undo.checkpoint(&field, true);
        field.new_line();
        field.type_str("y=2");
        assert_eq!(field.line_count(), 2);

        assert!(undo.undo(&mut field));
        assert_eq!(field.line_count(), 1);
        assert_eq!(field.to_latex(), "x=1");
    }
}
