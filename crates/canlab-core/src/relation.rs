//! A comparison between two expressions.
//!
//! `Expr` is a *value*: it has no operator that says how two values are
//! related. That is enough for an equation once the calling code folds the two
//! sides into `left − right`, because `= 0` is implied by the fold. It is not
//! enough for an inequality: `|x − 1| < 2`, `|x − 1| > 2` and `|x − 1| = 2` all
//! fold to the same difference, and no function of that difference can recover
//! which comparison was typed.
//!
//! So the comparison lives here, beside the expression rather than inside it.
//! This is syntax, not solving: nothing outside `canlab-core` reads a
//! [`Relation`] until the solver is asked. Keeping the value here is what lets
//! the input layer, the job queue and the solver agree on what was typed without
//! any of them inventing the other's half.

use crate::expr::Expr;

/// The four comparisons an inequality can use.
///
/// `=` deliberately has no variant: an equation already has a representation
/// (a difference that the solver sets to zero), and adding `Equal` here would
/// force every inequality caller to handle a case that is not an inequality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Comparator {
    /// `<`
    Less,
    /// `≤`
    LessOrEqual,
    /// `>`
    Greater,
    /// `≥`
    GreaterOrEqual,
}

impl Comparator {
    /// `true` for the two comparisons whose endpoints are not part of the set.
    ///
    /// A strict answer always writes a round bracket; a non-strict answer
    /// writes one only when the endpoint is not a solution for another reason
    /// (a pole, say), which is the caller's judgement and not this method's.
    pub fn is_strict(self) -> bool {
        matches!(self, Comparator::Less | Comparator::Greater)
    }
}

/// One comparison: `left < right`, `left ≥ right`, and so on.
///
/// Both sides are whole expressions, not a difference, so the comparator can
/// still be read after the value has crossed a job queue or a process boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    /// The expression on the left of the comparator.
    pub left: Expr,
    /// The expression on the right of the comparator.
    pub right: Expr,
    /// Which comparison stands between them.
    pub comparator: Comparator,
}

impl Relation {
    /// Build `left comparator right`.
    pub fn new(left: Expr, right: Expr, comparator: Comparator) -> Self {
        Self {
            left,
            right,
            comparator,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{int, sym};

    #[test]
    fn a_relation_keeps_both_sides_and_the_comparator() {
        let relation = Relation::new(sym("x"), int(2), Comparator::Less);
        assert_eq!(relation.left, sym("x"));
        assert_eq!(relation.right, int(2));
        assert_eq!(relation.comparator, Comparator::Less);
    }

    #[test]
    fn relations_compare_by_value_not_by_identity() {
        // Two separately built relations that say the same thing are equal:
        // the job queue and the tests compare them, not their addresses.
        let a = Relation::new(sym("x"), int(2), Comparator::GreaterOrEqual);
        let b = Relation::new(sym("x"), int(2), Comparator::GreaterOrEqual);
        assert_eq!(a, b);

        // The comparator is part of the value. If it were not, the whole reason
        // this type exists would be gone.
        let less = Relation::new(sym("x"), int(2), Comparator::Less);
        assert_ne!(a, less);
    }

    #[test]
    fn only_less_and_greater_are_strict() {
        assert!(Comparator::Less.is_strict());
        assert!(Comparator::Greater.is_strict());
        assert!(!Comparator::LessOrEqual.is_strict());
        assert!(!Comparator::GreaterOrEqual.is_strict());
    }
}
