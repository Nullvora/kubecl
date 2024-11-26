use crate::{codegen::Compiler, compute::CubeTask, ir::Elem};
use cubecl_runtime::{channel::ComputeChannel, client::ComputeClient, server::ComputeServer};

pub use cubecl_runtime::channel;
pub use cubecl_runtime::client;
pub use cubecl_runtime::server;
pub use cubecl_runtime::tune;
pub use cubecl_runtime::ExecutionMode;

/// Runtime for the CubeCL.
pub trait Runtime: Send + Sync + 'static + core::fmt::Debug {
    /// The compiler used to compile the inner representation into tokens.
    type Compiler: Compiler;
    /// The compute server used to run kernels and perform autotuning.
    type Server: ComputeServer<Kernel = Box<dyn CubeTask<Self::Compiler>>, Feature = Feature>;
    /// The channel used to communicate with the compute server.
    type Channel: ComputeChannel<Self::Server>;
    /// The device used to retrieve the compute client.
    type Device: Default + Clone + core::fmt::Debug;

    /// Retrieve the compute client from the runtime device.
    fn client(device: &Self::Device) -> ComputeClient<Self::Server, Self::Channel>;

    /// The runtime name.
    fn name() -> &'static str;

    /// The default extension for the runtime's kernel/shader code.
    /// Might change based on which compiler is used.
    fn extension() -> &'static str;

    /// Return true if global input array lengths should be added to kernel info.
    fn require_array_lengths() -> bool {
        false
    }

    /// Returns the supported line sizes for the current runtime's compiler.
    fn supported_line_sizes() -> &'static [u8];
    /// Returns the maximum cube count on each dimension that can be launched.
    fn max_cube_count() -> (u32, u32, u32);
}

/// Every feature that can be supported by a [cube runtime](Runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Feature {
    /// The plane feature enables all basic warp/subgroup operations.
    Plane,
    /// The cmma feature enables cooperative matrix-multiply and accumulate operations.
    Cmma {
        a: Elem,
        b: Elem,
        c: Elem,
        m: u8,
        k: u8,
        n: u8,
    },
    CmmaWarpSize(i32),
    Type(Elem),
}
