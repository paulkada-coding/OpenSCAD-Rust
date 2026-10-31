//! OpenSCAD language evaluator: runs a parsed program and produces the
//! geometry tree ([`geometry::csg::Node`]) plus echo/warning messages.

mod builtins;
mod eval;
mod message;
mod print;
mod scope;
mod value;

pub use eval::{EvalOptions, Evaluator, SourceInfo};
pub use message::{EvalError, EvalResult, Level, Message};
pub use value::{Arguments, RangeValue, Value};

#[cfg(test)]
mod tests;
