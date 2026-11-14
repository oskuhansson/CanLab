//! `canlab-core` — the expression representation at the centre of the system.
//!
//! Design rules that the rest of the crate depends on:
//!
//! 1. [`Expr`] is an `Arc<ExprNode>`: cloning is a refcount bump, never a deep
//!    copy. Rewrites share almost all of the old tree.
//! 2. Raw constructors ([`ExprNode::add_raw`] and friends) never normalize.
//!    Smart constructors ([`normalize::add`] and friends) always do. Keeping
//!    the two apart is what stops normalization from becoming recursive and
//!    quadratic.
//! 3. Numbers are exact until something explicitly asks for a float. `2` and
//!    `2.0` are different values and both survive a round trip.

pub mod approx;
pub mod eval;
pub mod expr;
pub mod lua;
pub mod normalize;
pub mod rewrite;
pub mod number;
pub mod quantity;
pub mod sigfig;
pub mod relation;
pub mod symbol;
pub mod wgsl;

pub mod diff;
pub mod dimension;
pub use diff::{differentiate, differentiate_n};
pub use eval::{INLINE_STACK, Op, Program};
pub use expr::{Expr, ExprNode, FuncId, Hold, int, ratio, sym};
pub use normalize::{add, mul, normalize, pow};
pub use number::Number;
pub use relation::{Comparator, Relation};
pub use symbol::SymbolId;
