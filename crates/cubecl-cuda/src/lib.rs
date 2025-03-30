#[macro_use]
extern crate derive_new;
extern crate alloc;

mod compute;
mod device;
mod runtime;

pub use device::*;
pub use runtime::*;

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    pub type TestRuntime = crate::CudaRuntime;
    pub use half::{bf16, f16};

    cubecl_core::testgen_all!(f32: [f16, bf16, f32, f64], i32: [i8, i16, i32, i64], u32: [u8, u16, u32, u64]);
    cubecl_linalg::testgen_matmul_accelerated!([f16]);
    cubecl_linalg::testgen_matmul_tma!([f16]);
    cubecl_linalg::testgen_matmul_quantized!();
    cubecl_linalg::testgen_matmul_simple!([f16, bf16, f32]);
    cubecl_linalg::testgen_matmul_tiling2d!([f16, bf16, f32]);
    cubecl_linalg::testgen_tensor_identity!([f16, bf16, f32, u32]);
    cubecl_reduce::testgen_reduce!([f16, bf16, f32, f64]);
    cubecl_reduce::testgen_shared_sum!([f16, bf16, f32, f64]);
}
