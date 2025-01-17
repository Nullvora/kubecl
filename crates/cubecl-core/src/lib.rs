extern crate alloc;

#[macro_use]
extern crate derive_new;

/// Cube Frontend Types.
pub mod frontend;

/// Some future utilities that work across environments.
pub use cubecl_common::future;

pub use cubecl_runtime::memory_management::MemoryConfiguration;
pub use frontend::cmma;

/// Cube Language Internal Representation.
pub mod ir;

pub mod codegen;
pub mod compute;
pub mod prelude;

mod pod;
mod runtime;

pub use codegen::*;
pub use pod::*;
pub use runtime::*;

pub use cubecl_macros::*;
pub use cubecl_runtime::benchmark;
pub use cubecl_runtime::memory_management::MemoryUsage;

/// An approximation of the plane dimension.
pub const PLANE_DIM_APPROX: usize = 16;

use crate::ir::KernelDefinition;
use frontend::LaunchArg;

pub use prelude::CubeCount;
pub use prelude::CubeDim;
pub use prelude::{flex32, tf32};

mod id;
pub use id::*;

/// Implement this trait to create a [kernel definition](KernelDefinition).
pub trait Kernel: Send + Sync + 'static + Sized {
    /// Convert to a kernel definition.
    fn define(&self) -> KernelDefinition;
    /// Identifier for the kernel, used for caching kernel compilation.
    fn id(&self) -> KernelId {
        KernelId::new::<Self>()
    }
}

/// Calculate the number of cubes required to execute an operation where one cube unit is
/// assigned to one element.
pub fn calculate_cube_count_elemwise(num_elems: usize, cube_dim: CubeDim) -> CubeCount {
    let num_elems_per_cube = cube_dim.num_elems();
    let cube_counts = f32::max(1.0, f32::ceil(num_elems as f32 / num_elems_per_cube as f32));
    let cube_count_x = f32::ceil(f32::sqrt(cube_counts));
    let cube_count_y = f32::ceil(num_elems as f32 / (cube_count_x * num_elems_per_cube as f32));

    CubeCount::Static(cube_count_x as u32, cube_count_y as u32, 1)
}

pub fn tensor_vectorization_factor(
    factors: &[u8],
    shape: &[usize],
    strides: &[usize],
    dim: usize,
) -> u8 {
    tensor_line_size_parallel(factors.iter().cloned(), shape, strides, dim)
}
pub fn tensor_line_size(factors: &[u8], shape: &[usize], strides: &[usize], dim: usize) -> u8 {
    tensor_line_size_parallel(factors.iter().cloned(), shape, strides, dim)
}

/// Find the maximum line size usable for parallel vectorization along the given axis
/// from the supported line sizes or return 1 if vectorization is impossible.
///
/// This function is designed to never return a line size above 1 by error,
/// but doesn't guarantee to always return the actual maximum possible line size.
/// That is, it may be overly strict.
///
/// Currently, this checks that the stride of the axis is 1, that it's shape is
/// divisible by a candidate line size and that the smallest stride that is not 1
/// is equal to the shape of the axis.
/// The last condition ensure that the current axis is contiguous within the next stride.
pub fn tensor_line_size_parallel(
    supported_line_sizes: impl Iterator<Item = u8>,
    shape: &[usize],
    strides: &[usize],
    axis: usize,
) -> u8 {
    match strides.get(axis) {
        Some(val) => {
            if *val != 1 {
                return 1;
            }
        }
        None => return 1,
    }

    let axis_shape = match shape.get(axis) {
        Some(val) => val,
        None => return 1,
    };

    let next_stride = strides.iter().filter(|stride| **stride > 1).min();

    if let Some(next_stride) = next_stride {
        if next_stride != axis_shape {
            return 1;
        }
    }

    supported_line_sizes
        .filter(|line_size| axis_shape % *line_size as usize == 0)
        .max()
        .unwrap_or(1)
}

/// Find the maximum line size usable for perpendicular vectorization along the given axis
/// from the supported line sizes or return 1 if vectorization is impossible.
///
/// This function is designed to never return a line size above 1 by error,
/// but doesn't guarantee to always return the actual maximum possible line size.
/// That is, it may be overly strict.
///
/// Currently, this checks that the stride of the axis is divisible by a candidate line size
/// and that the product of all shapes of axes with smaller strides is equal to the stride of the axis.
/// The second condition ensure that elements within the stride are contiguous.
pub fn tensor_line_size_perpendicular(
    supported_line_sizes: impl Iterator<Item = u8>,
    shape: &[usize],
    strides: &[usize],
    axis: usize,
) -> u8 {
    let axis_stride = match strides.get(axis) {
        Some(stride) => *stride,
        None => return 1,
    };

    let prod_shape_axes_smaller_strides = strides
        .iter()
        .zip(shape.iter())
        .filter(|(stride, _)| **stride < axis_stride)
        .map(|(_, shape)| shape)
        .product::<usize>();

    if axis_stride != prod_shape_axes_smaller_strides {
        return 1;
    }

    supported_line_sizes
        .filter(|line_size| axis_stride % *line_size as usize == 0)
        .max()
        .unwrap_or(1)
}

/// Runtime arguments to launch a kernel.
pub type RuntimeArg<'a, T, R> = <T as LaunchArg>::RuntimeArg<'a, R>;

#[cfg(feature = "export_tests")]
/// Tests only useful for runtimes.
pub mod runtime_tests;
