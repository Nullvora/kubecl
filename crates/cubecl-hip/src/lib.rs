#[allow(unused_imports)]
#[macro_use]
extern crate derive_new;
extern crate alloc;

#[cfg(target_os = "linux")]
pub mod compute;
#[cfg(target_os = "linux")]
pub mod device;
#[cfg(target_os = "linux")]
pub mod runtime;
#[cfg(target_os = "linux")]
pub use device::*;
#[cfg(target_os = "linux")]
pub use runtime::HipRuntime;
#[cfg(target_os = "linux")]
#[cfg(feature = "rocwmma")]
pub(crate) type HipWmmaCompiler = cubecl_cpp::hip::wmma::RocWmmaCompiler;
#[cfg(target_os = "linux")]
#[cfg(not(feature = "rocwmma"))]
pub(crate) type HipWmmaCompiler = cubecl_cpp::hip::wmma::WmmaIntrinsicCompiler;

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use half::{bf16, f16};
    pub type TestRuntime = crate::HipRuntime;

    cubecl_core::testgen_all!();
    cubecl_linalg::testgen_cmma_matmul!();
    cubecl_reduce::testgen_reduce!([f16, bf16, f32, f64]);
}
