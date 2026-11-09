//! Where the cursor is.
//!
//! A cursor is a *path into the tree* plus an offset in the innermost row. There is
//! no single integer index that could address "inside the denominator", which is
//! exactly the property a plain text box lacks.

use crate::tree::Slot;

/// One step of a cursor path: "child `index` of the current row, argument `slot`".
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct PathStep {
    /// Index into the enclosing [`crate::Row`].
    pub index: usize,
    /// Which argument of that node the path descends into.
    pub slot: Slot,
}

impl PathStep {
    pub fn new(index: usize, slot: Slot) -> PathStep {
        PathStep { index, slot }
    }
}

/// A position between two children of a row, identified structurally.
///
/// `path.is_empty()` means "in the root row". Two cursors that look identical on
/// screen can be different positions: the start of a numerator and the point just
/// before the whole fraction are the same pixel and different `Cursor`s, and
/// Left/Right distinguish them. That is the observed behaviour.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Cursor {
    /// Slot path from the root, outermost first.
    pub path: Vec<PathStep>,
    /// Index within the innermost row: `0` is before the first child.
    pub offset: usize,
}

impl Cursor {
    /// The root position before the first child.
    pub fn root() -> Cursor {
        Cursor { path: Vec::new(), offset: 0 }
    }

    /// A position at `offset` in the given slot path.
    pub fn at(path: Vec<PathStep>, offset: usize) -> Cursor {
        Cursor { path, offset }
    }

    /// True when the cursor is not in the root row.
    pub fn is_nested(&self) -> bool {
        !self.path.is_empty()
    }

    /// The innermost slot the cursor is inside, if any.
    pub fn slot(&self) -> Option<Slot> {
        self.path.last().map(|s| s.slot)
    }

    /// This cursor extended by one step into a slot.
    pub fn pushed(&self, index: usize, slot: Slot) -> Cursor {
        let mut path = self.path.clone();
        path.push(PathStep::new(index, slot));
        Cursor { path, offset: 0 }
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor::root()
    }
}

/// A contiguous selection, stored as an ordered cursor pair.
///
/// Only selections that live in one row are supported; that covers `Ctrl-A` and
/// dragging across a single construct, which is what was observed. Cross-row drag
/// semantics are not reproduced (see `docs/mathquill-interaction.md` §10).
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Selection {
    /// Earlier position.
    pub start: Cursor,
    /// Later position; `end` is always at the same path as `start` and
    /// `end.offset >= start.offset`.
    pub end: Cursor,
}

impl Selection {
    /// Normalise two cursors into an ordered selection, if they share a row.
    pub fn between(a: Cursor, b: Cursor) -> Option<Selection> {
        if a.path != b.path {
            return None;
        }
        if a.offset <= b.offset {
            Some(Selection { start: a, end: b })
        } else {
            Some(Selection { start: b, end: a })
        }
    }

    /// True when the selection covers no whole node.
    pub fn is_collapsed(&self) -> bool {
        self.start.offset == self.end.offset
    }
}
