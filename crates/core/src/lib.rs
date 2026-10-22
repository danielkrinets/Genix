//! Ядро параметрического CAD: выражения, чертежи, операции над телами, документ с историей.

pub mod csg;
pub mod dxf;
pub mod doc;
pub mod expr;
pub mod geom;
pub mod mesh;
pub mod sketch;
pub mod trace;
mod sketch_ops;

pub use doc::{Document, Evaluator, Feature, FeatureKind, Op};
pub use expr::{Expr, Params};
pub use mesh::Mesh;
pub use sketch::{Constraint, Entity, Plane, Region, Sketch};

thread_local! {
    static GUARDED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs `f`, turning a panic into an `Err`. While it runs, `panic_is_guarded()` is true, so a crash-report hook can tell
/// a panic somebody is going to handle (a fillet that failed on extreme values) from one that really ends the program.
pub fn guarded<R>(f: impl FnOnce() -> R + std::panic::UnwindSafe) -> std::thread::Result<R> {
    GUARDED.with(|g| g.set(g.get() + 1));
    let r = std::panic::catch_unwind(f);
    GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
    r
}

pub fn panic_is_guarded() -> bool {
    GUARDED.with(|g| g.get() > 0)
}
