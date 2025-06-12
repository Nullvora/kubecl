mod config;
mod matmul;
mod setup;

pub use config::*;
pub use matmul::*;

use crate::components::global::load::sync_full_ordered;

/// The ordered double buffering global matmul
/// requires tilewise loading on `Lhs` to guarantee that planes
/// only use data they have loaded themselves.
pub type LL = sync_full_ordered::LoadingStrategy;
