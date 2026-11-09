//! Interned symbol names.
//!
//! Comparing `SymbolId`s is a `u32` compare instead of a string compare, and
//! the interner makes `Expr` hashing cheap. The table is process-global and
//! append-only, which is fine: a CAS session does not create unbounded names.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SymbolId(u32);

struct Interner {
    map: HashMap<Arc<str>, u32>,
    names: Vec<Arc<str>>,
}

fn interner() -> &'static Mutex<Interner> {
    static INTERNER: OnceLock<Mutex<Interner>> = OnceLock::new();
    INTERNER.get_or_init(|| {
        Mutex::new(Interner {
            map: HashMap::new(),
            names: Vec::new(),
        })
    })
}

impl SymbolId {
    pub fn intern(name: &str) -> Self {
        let mut guard = interner().lock().expect("symbol interner poisoned");
        if let Some(id) = guard.map.get(name) {
            return SymbolId(*id);
        }
        let id = guard.names.len() as u32;
        let name: Arc<str> = Arc::from(name);
        guard.map.insert(name.clone(), id);
        guard.names.push(name);
        SymbolId(id)
    }

    pub fn name(self) -> Arc<str> {
        let guard = interner().lock().expect("symbol interner poisoned");
        guard.names[self.0 as usize].clone()
    }

    pub fn as_str(self) -> String {
        self.name().to_string()
    }

    pub fn idx(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for SymbolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_stable() {
        let a = SymbolId::intern("x");
        let b = SymbolId::intern("x");
        assert_eq!(a, b);
        assert_eq!(a.name().as_ref(), "x");
        assert_ne!(SymbolId::intern("y"), a);
    }
}
