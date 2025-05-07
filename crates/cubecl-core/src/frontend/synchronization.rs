use crate::{
    ir::{Scope, Synchronization},
    unexpanded,
};

// Among all backends, the memory order guarantee of WebGPU is the weakest
// So Cubecl's memory order cannot be stronger than that of WebGPU

/// # Coordinates the following among all invocations in the current cube:
///
/// * Memory writes to variables in cube address space(shared memory) complete,
///   e.g. writes that were initiated actually land in the cube address space memory.
///
/// * Then all the invocations in the cube wait for each other to arrive at the barrier, i.e. this step.
///
/// * Then all the invocations int the cube begin executing after the barrier, and all writes to cube address space made before the barrier are now visible to any invocation in this cube.
pub fn sync_cube() {}

pub mod sync_cube {
    use super::*;

    pub fn expand(scope: &mut Scope) {
        scope.register(Synchronization::SyncCube)
    }
}

/// Synchronizes threads within their plane (e.g., warp or SIMD group).
///
/// Not all targets support plane-level synchronization. Use [`SyncPlaneMode`]
/// to control whether a fallback to cube-wide synchronization is allowed
/// when plane sync is unavailable.
pub fn sync_plane(_sync_plane_mode: SyncPlaneMode) {
    unexpanded!()
}

pub mod sync_plane {
    use super::*;

    pub fn expand(scope: &mut Scope, sync_plane_mode: SyncPlaneMode) {
        match sync_plane_mode {
            SyncPlaneMode::Strict => scope.register(Synchronization::SyncPlaneStrict),
            SyncPlaneMode::Fallback => scope.register(Synchronization::SyncPlaneFallback),
        }
    }
}

/// Determines whether a sync plane is allowed to fall back to a sync cube
/// when native support is unavailable.
///
/// In some cases, falling back may only incur a minor performance cost.
/// In others, it can break correctness or even cause deadlocks —
/// especially when warp specialization is involved.
pub enum SyncPlaneMode {
    Strict,   // Requires native warp sync, or fails at kernel compilation
    Fallback, // Allows fallback to Cube-wide sync, may break correctness
}

/// * Sync_storage is the same but change "cube address space(shared memory)" to "storage address space(input args)". But the set of invocations that are collaborating is still only the invocations in the same cube.
///
/// * There is no guarantee about using barriers alone to make the writes to storage buffer in one cube become visible to invocations in a different cube.
pub fn sync_storage() {}

pub mod sync_storage {
    use super::*;

    pub fn expand(scope: &mut Scope) {
        scope.register(Synchronization::SyncStorage)
    }
}

/// `sync_proxy_shared` is a synchronization fence for the experimental SM 9.0+ CTA proxy functions
/// (i.e. TMA tensor copy). Experimental and subject to change.
pub fn sync_proxy_shared() {
    unexpanded!()
}

pub mod sync_proxy_shared {
    use super::*;

    pub fn expand(scope: &mut Scope) {
        scope.register(Synchronization::SyncProxyShared)
    }
}
