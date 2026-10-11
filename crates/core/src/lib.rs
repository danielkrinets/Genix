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
