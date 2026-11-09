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

pub mod eval;
pub mod expr;
pub mod normalize;
pub mod number;
pub mod symbol;

pub use eval::{Op, Program};
pub use expr::{Expr, ExprNode, FuncId, Hold, int, ratio, sym};
pub use normalize::{add, mul, normalize, pow};
pub use number::Number;
pub use symbol::SymbolId;
