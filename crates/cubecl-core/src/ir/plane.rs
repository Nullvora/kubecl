use std::fmt::Display;

use super::{BinaryOperator, UnaryOperator};
use serde::{Deserialize, Serialize};

/// All plane operations.
///
/// Note that not all backends support plane (warp/subgroup) operations. Use the [runtime flag](crate::Feature::Plane).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(dead_code, missing_docs)] // Some variants might not be used with different flags
pub enum Plane {
    Elect,
    All(UnaryOperator),
    Any(UnaryOperator),
    Broadcast(BinaryOperator),
    Sum(UnaryOperator),
    Prod(UnaryOperator),
    Min(UnaryOperator),
    Max(UnaryOperator),
}

impl Display for Plane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Plane::Elect => writeln!(f, "plane_elect()"),
            Plane::All(op) => writeln!(f, "plane_all({})", op.input),
            Plane::Any(op) => writeln!(f, "plane_any({})", op.input),
            Plane::Broadcast(op) => {
                writeln!(f, "plane_broadcast({}, {})", op.lhs, op.rhs)
            }
            Plane::Sum(op) => writeln!(f, "plane_sum({})", op.input),
            Plane::Prod(op) => writeln!(f, "plane_product({})", op.input),
            Plane::Min(op) => writeln!(f, "plane_min({})", op.input),
            Plane::Max(op) => writeln!(f, "plane_max({})", op.input),
        }
    }
}
