#![allow(missing_docs)]

use cubecl_core as cubecl;
use cubecl_core::prelude::*;
use rand::{
    distributions::{Distribution, Uniform},
    rngs::StdRng,
    SeedableRng,
};

use crate::{
    reduce_naive, reduce_plane, reduce_shared, ArgMax, ArgMin, Mean, Prod, ReduceNaiveInstruction,
    ReducePlaneInstruction, ReduceSharedInstruction, Sum,
};

// All random values generated for tests will be in the set
// {-1, -1 + E, -1 + 2E, ..., 1 - E, 1} with E = 1 / PRECISION.
// We choose this set to avoid precision issues with f16 and bf16 and
// also to add multiple similar values to properly test ArgMax and ArgMin.
const PRECISION: i32 = 4;

// Simple kernel to launch tests.
#[cube(launch_unchecked)]
pub fn kernel_reduce_naive<I: Numeric, O: Numeric, R: ReduceNaiveInstruction<I>>(
    input: &Tensor<Line<I>>,
    output: &mut Tensor<Line<O>>,
    dim: u32,
) {
    reduce_naive::<R, I, O>(input, output, dim)
}

// Simple kernel to launch tests.
#[cube(launch_unchecked)]
pub fn kernel_reduce_shared<I: Numeric, O: Numeric, R: ReduceSharedInstruction<I>>(
    input: &Tensor<Line<I>>,
    output: &mut Tensor<Line<O>>,
    reduce_dim: u32,
    #[comptime] cube_dim: u32,
    #[comptime] exact_shape: bool,
) {
    reduce_shared::<R, I, O>(input, output, reduce_dim, cube_dim, exact_shape)
}

// Simple kernel to launch tests.
#[cube(launch_unchecked)]
pub fn kernel_reduce_plane<I: Numeric, O: Numeric, R: ReducePlaneInstruction<I>>(
    input: &Tensor<Line<I>>,
    output: &mut Tensor<Line<O>>,
    reduce_dim: u32,
    #[comptime] cube_dim: u32,
    #[comptime] exact_shape: bool,
) {
    reduce_plane::<R, I, O>(input, output, reduce_dim, cube_dim, exact_shape)
}

// This macro generate all the tests.
#[macro_export]
macro_rules! testgen_reduce {
    // Generate all the tests for a list of types.
    ([$($float:ident), *]) => {
        mod test_reduce {
            use super::*;
            ::paste::paste! {
                $(mod [<$float _ty>] {
                    use super::*;

                    $crate::testgen_reduce!($float);
                })*
            }
        }
    };

    // Generate all the tests for a specific float type.
    ($float:ident) => {
        use cubecl_reduce::test::TestCase;
        use cubecl_core::prelude::CubeCount;

        $crate::impl_test_reduce!(
            naive,
            $float,
            [
                {
                    id: "reduce_columns_small_matrix_row_major",
                    shape: [4, 8],
                    stride: [8, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_single(),
                    cube_dim: CubeDim::new_2d(4, 8),
                    line_size: 1,
                },
                {
                    id: "reduce_columns_large_matrix_row_major",
                    shape: [8, 256],
                    stride: [256, 1],
                    reduce_dim: 1,
                    cube_count: CubeCount::new_1d(8),
                    cube_dim: CubeDim::new_2d(16, 16),
                    line_size: 1,
                },
                {
                    id: "reduce_rows_large_matrix_row_major",
                    shape: [8, 256],
                    stride: [256, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_1d(8),
                    cube_dim: CubeDim::new_2d(16, 16),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor",
                    shape: [16, 16, 16],
                    stride: [1, 256, 16],
                    reduce_dim: 2,
                    cube_count: CubeCount::new_1d(4),
                    cube_dim: CubeDim::new_2d(16, 16),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor_unexact_shape",
                    shape: [11, 12, 13],
                    stride: [156, 13, 1],
                    reduce_dim: 1,
                    cube_count: CubeCount::new_1d(4),
                    cube_dim: CubeDim::new_2d(16, 16),
                    line_size: 1,
                },
                {
                    id: "reduce_rows_large_matrix_row_major_line_size_four",
                    shape: [32, 64],
                    stride: [64, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_1d(8),
                    cube_dim: CubeDim::new_2d(16, 16),
                    line_size: 4,
                }
            ]
        );

        $crate::impl_test_reduce!(
            shared,
            $float,
            [
                {
                    id: "reduce_columns_small_matrix_row_major",
                    shape: [4, 8],
                    stride: [8, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_1d(8),
                    cube_dim: CubeDim::new_1d(2),
                    line_size: 1,
                },
                {
                    id: "reduce_columns_large_matrix_row_major",
                    shape: [8, 256],
                    stride: [256, 1],
                    reduce_dim: 1,
                    cube_count: CubeCount::new_1d(8),
                    cube_dim: CubeDim::new_1d(16),
                    line_size: 1,
                },
                {
                    id: "reduce_rows_large_matrix_row_major_non_power_two_cube_dim",
                    shape: [16, 256],
                    stride: [256, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_1d(256),
                    cube_dim: CubeDim::new_1d(5),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor",
                    shape: [16, 16, 16],
                    stride: [1, 256, 16],
                    reduce_dim: 2,
                    cube_count: CubeCount::new_2d(16, 16),
                    cube_dim: CubeDim::new_1d(4),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor_unexact_shape",
                    shape: [11, 12, 13],
                    stride: [156, 13, 1],
                    reduce_dim: 1,
                    cube_count: CubeCount::new_2d(11, 13),
                    cube_dim: CubeDim::new_1d(2),
                    line_size: 1,
                },
                {
                    id: "reduce_rows_large_matrix_row_major_line_size_four",
                    shape: [32, 64],
                    stride: [64, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_1d(64),
                    cube_dim: CubeDim::new_1d(8),
                    line_size: 4,
                }
            ]
        );

        $crate::impl_test_reduce!(
            plane,
            $float,
            [
                {
                    id: "reduce_columns_large_matrix_row_major",
                    shape: [8, 256],
                    stride: [256, 1],
                    reduce_dim: 1,
                    cube_count: CubeCount::new_1d(2),
                    cube_dim: CubeDim::new_1d(4 * 32),
                    line_size: 1,
                },
                {
                    id: "reduce_rows_large_matrix_row_major",
                    shape: [256, 8],
                    stride: [8, 1],
                    reduce_dim: 0,
                    cube_count: CubeCount::new_1d(2),
                    cube_dim: CubeDim::new_1d(4 * 32),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor_single_cube",
                    shape: [16, 2, 128],
                    stride: [1, 16 * 128, 16],
                    reduce_dim: 2,
                    cube_count: CubeCount::new_1d(1),
                    cube_dim: CubeDim::new_1d(32*32),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor_multiple_cubes",
                    shape: [16, 2, 128],
                    stride: [1, 16 * 128, 16],
                    reduce_dim: 2,
                    cube_count: CubeCount::new_1d(8),
                    cube_dim: CubeDim::new_1d(32*4),
                    line_size: 1,
                },
                {
                    id: "rank_three_tensor_unexact_shape",
                    shape: [3, 5, 59],
                    stride: [5*59, 59, 1],
                    reduce_dim: 2,
                    cube_count: CubeCount::new_1d(3),
                    cube_dim: CubeDim::new_1d(5 * 32),
                    line_size: 1,
                },
                {
                    id: "reduce_rows_large_matrix_row_major_line_size_four",
                    shape: [16, 1024],
                    stride: [1024, 1],
                    reduce_dim: 1,
                    cube_count: CubeCount::new_1d(4),
                    cube_dim: CubeDim::new_1d(32*4),
                    line_size: 4,
                }
            ]
        );
    };
}

// For a given tensor description and cube settings
// run the tests for `ReduceSum`, `ReduceProd`, `ReduceMean`, `ReduceArgMax` and `ReduceArgMin`
// for all implementations.
// For each test, a reference reduction is computed on the CPU to compare the outcome of the kernel.
#[macro_export]
macro_rules! impl_test_reduce {
    (
        $kind:ident,
        $float:ident,
        [
            $(
                {
                    id: $id:literal,
                    shape: $shape:expr,
                    stride: $stride:expr,
                    reduce_dim: $reduce_dim:expr,
                    cube_count: $cube_count:expr,
                    cube_dim: $cube_dim:expr,
                    line_size: $line_size:expr,
                }
            ),*
        ]) => {
        ::paste::paste! {
            $(
                #[test]
                pub fn [< reduce_sum_ $kind _ $id >]() {
                    let test = TestCase {
                        shape: $shape.into(),
                        stride: $stride.into(),
                        reduce_dim: $reduce_dim,
                        cube_count: $cube_count,
                        cube_dim: $cube_dim,
                        line_size:$line_size
                    };
                    test.[< test_sum_ $kind >]::<$float, TestRuntime>(&Default::default());
                }

                #[test]
                pub fn [< reduce_prod_ $kind _ $id >]() {
                    let test = TestCase {
                        shape: $shape.into(),
                        stride: $stride.into(),
                        reduce_dim: $reduce_dim,
                        cube_count: $cube_count,
                        cube_dim: $cube_dim,
                        line_size:$line_size
                    };
                    test.[< test_prod_ $kind >]::<$float, TestRuntime>(&Default::default());
                }

                #[test]
                pub fn [< reduce_mean_ $kind _ $id >]() {
                    let test = TestCase {
                        shape: $shape.into(),
                        stride: $stride.into(),
                        reduce_dim: $reduce_dim,
                        cube_count: $cube_count,
                        cube_dim: $cube_dim,
                        line_size:$line_size
                    };
                    test.[< test_mean_ $kind >]::<$float, TestRuntime>(&Default::default());
                }

                #[test]
                pub fn [< reduce_argmax_ $kind _ $id >]() {
                    let test = TestCase {
                        shape: $shape.into(),
                        stride: $stride.into(),
                        reduce_dim: $reduce_dim,
                        cube_count: $cube_count,
                        cube_dim: $cube_dim,
                        line_size:$line_size
                    };
                    test.[< test_argmax_ $kind >]::<$float, TestRuntime>(&Default::default());
                }

                #[test]
                pub fn [< reduce_argmin_ $kind _ $id >]() {
                    let test = TestCase {
                        shape: $shape.into(),
                        stride: $stride.into(),
                        reduce_dim: $reduce_dim,
                        cube_count: $cube_count,
                        cube_dim: $cube_dim,
                        line_size:$line_size
                    };
                    test.[< test_argmin_ $kind >]::<$float, TestRuntime>(&Default::default());
                }
            )*
        }
    };
}

#[derive(Debug)]
pub struct TestCase {
    pub shape: Vec<usize>,
    pub stride: Vec<usize>,
    pub reduce_dim: u32,
    pub line_size: u8,
    pub cube_count: CubeCount,
    pub cube_dim: CubeDim,
}

impl TestCase {
    pub fn test_sum_naive<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_sum(&input_values);
        self.run_test_naive::<F, F, R, Sum>(device, input_values, expected_values)
    }

    pub fn test_prod_naive<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_prod(&input_values);
        self.run_test_naive::<F, F, R, Prod>(device, input_values, expected_values)
    }

    pub fn test_mean_naive<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_mean(&input_values);
        self.run_test_naive::<F, F, R, Mean>(device, input_values, expected_values)
    }

    pub fn test_argmax_naive<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_argmax(&input_values);
        self.run_test_naive::<F, u32, R, ArgMax>(device, input_values, expected_values)
    }

    pub fn test_argmin_naive<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_argmin(&input_values);
        self.run_test_naive::<F, u32, R, ArgMin>(device, input_values, expected_values)
    }

    pub fn run_test_naive<I, O, R, K>(
        &self,
        device: &R::Device,
        input_values: Vec<I>,
        expected_values: Vec<O>,
    ) where
        I: Numeric + CubeElement + std::fmt::Display,
        O: Numeric + CubeElement + std::fmt::Display,
        R: Runtime,
        K: ReduceNaiveInstruction<I>,
    {
        let client = R::client(device);

        let input_handle = client.create(I::as_bytes(&input_values));

        // Zero initialize a tensor with the same shape as input
        // except for the `self.reduce_dim` axis where the shape is 1.
        let output_handle =
            client.create(O::as_bytes(&vec![O::from_int(0); expected_values.len()]));
        let mut output_shape = self.shape.clone();
        output_shape[self.reduce_dim as usize] = 1;
        let output_stride = self.output_stride();

        unsafe {
            let input_tensor = TensorArg::from_raw_parts::<I>(
                &input_handle,
                &self.stride,
                &self.shape,
                self.line_size,
            );
            let output_tensor = TensorArg::from_raw_parts::<O>(
                &output_handle,
                &output_stride,
                &output_shape,
                self.line_size,
            );

            kernel_reduce_naive::launch_unchecked::<I, O, K, R>(
                &client,
                self.cube_count.clone(),
                self.cube_dim,
                input_tensor,
                output_tensor,
                ScalarArg::new(self.reduce_dim),
            );
        }

        let binding = output_handle.binding();
        let bytes = client.read_one(binding);
        let output_values = O::from_bytes(&bytes);

        assert_approx_equal(output_values, &expected_values);
    }

    pub fn test_sum_shared<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_sum(&input_values);
        self.run_test_shared::<F, F, R, Sum>(device, input_values, expected_values)
    }

    pub fn test_prod_shared<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_prod(&input_values);
        self.run_test_shared::<F, F, R, Prod>(device, input_values, expected_values)
    }

    pub fn test_mean_shared<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_mean(&input_values);
        self.run_test_shared::<F, F, R, Mean>(device, input_values, expected_values)
    }

    pub fn test_argmax_shared<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_argmax(&input_values);
        self.run_test_shared::<F, u32, R, ArgMax>(device, input_values, expected_values)
    }

    pub fn test_argmin_shared<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_argmin(&input_values);
        self.run_test_shared::<F, u32, R, ArgMin>(device, input_values, expected_values)
    }

    pub fn run_test_shared<I, O, R, K>(
        &self,
        device: &R::Device,
        input_values: Vec<I>,
        expected_values: Vec<O>,
    ) where
        I: Numeric + CubeElement + std::fmt::Display,
        O: Numeric + CubeElement + std::fmt::Display,
        R: Runtime,
        K: ReduceSharedInstruction<I>,
    {
        let client = R::client(device);
        let input_handle = client.create(I::as_bytes(&input_values));

        // Zero initialize a tensor with the same shape as input
        // except for the `self.reduce_dim` axis where the shape is 1.
        let output_handle =
            client.create(O::as_bytes(&vec![O::from_int(0); expected_values.len()]));
        let mut output_shape = self.shape.clone();
        output_shape[self.reduce_dim as usize] = 1;
        let output_stride = self.output_stride();

        let exact_shape =
            self.shape[self.reduce_dim as usize] % self.cube_dim.num_elems() as usize == 0;

        unsafe {
            let input_tensor = TensorArg::from_raw_parts::<I>(
                &input_handle,
                &self.stride,
                &self.shape,
                self.line_size,
            );
            let output_tensor = TensorArg::from_raw_parts::<O>(
                &output_handle,
                &output_stride,
                &output_shape,
                self.line_size,
            );

            kernel_reduce_shared::launch_unchecked::<I, O, K, R>(
                &client,
                self.cube_count.clone(),
                self.cube_dim,
                input_tensor,
                output_tensor,
                ScalarArg::new(self.reduce_dim),
                self.cube_dim.num_elems(),
                exact_shape,
            );
        }

        let binding = output_handle.binding();
        let bytes = client.read_one(binding);
        let output_values = O::from_bytes(&bytes);

        assert_approx_equal(output_values, &expected_values);
    }

    pub fn test_sum_plane<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_sum(&input_values);
        self.run_test_plane::<F, F, R, Sum>(device, input_values, expected_values)
    }

    pub fn test_prod_plane<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_prod(&input_values);
        self.run_test_plane::<F, F, R, Prod>(device, input_values, expected_values)
    }

    pub fn test_mean_plane<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_mean(&input_values);
        self.run_test_plane::<F, F, R, Mean>(device, input_values, expected_values)
    }

    pub fn test_argmax_plane<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_argmax(&input_values);
        self.run_test_plane::<F, u32, R, ArgMax>(device, input_values, expected_values)
    }

    pub fn test_argmin_plane<F, R>(&self, device: &R::Device)
    where
        F: Float + CubeElement + std::fmt::Display,
        R: Runtime,
    {
        let input_values: Vec<F> = self.random_input_values();
        let expected_values = self.cpu_argmin(&input_values);
        self.run_test_plane::<F, u32, R, ArgMin>(device, input_values, expected_values)
    }

    pub fn run_test_plane<I, O, R, K>(
        &self,
        device: &R::Device,
        input_values: Vec<I>,
        expected_values: Vec<O>,
    ) where
        I: Numeric + CubeElement + std::fmt::Display,
        O: Numeric + CubeElement + std::fmt::Display,
        R: Runtime,
        K: ReducePlaneInstruction<I>,
    {
        let client = R::client(device);

        // Check that planes are supported and that plane size is always 32.
        // The tests are designed for a plane size of 32.
        let plane_size = 32;
        let properties = client.properties().hardware_properties();
        if !client
            .properties()
            .feature_enabled(cubecl_core::Feature::Plane)
            || properties.plane_size_min != 32
            || properties.plane_size_max != 32
        {
            return;
        }

        let input_handle = client.create(I::as_bytes(&input_values));

        // Zero initialize a tensor with the same shape as input
        // except for the `self.reduce_dim` axis where the shape is 1.
        let output_handle =
            client.create(O::as_bytes(&vec![O::from_int(0); expected_values.len()]));
        let mut output_shape = self.shape.clone();
        output_shape[self.reduce_dim as usize] = 1;
        let output_stride = self.output_stride();

        let exact_shape = self.shape[self.reduce_dim as usize] % plane_size as usize == 0;

        unsafe {
            let input_tensor = TensorArg::from_raw_parts::<I>(
                &input_handle,
                &self.stride,
                &self.shape,
                self.line_size,
            );
            let output_tensor = TensorArg::from_raw_parts::<O>(
                &output_handle,
                &output_stride,
                &output_shape,
                self.line_size,
            );

            kernel_reduce_plane::launch_unchecked::<I, O, K, R>(
                &client,
                self.cube_count.clone(),
                self.cube_dim,
                input_tensor,
                output_tensor,
                ScalarArg::new(self.reduce_dim),
                self.cube_dim.num_elems(),
                exact_shape,
            );
        }

        let binding = output_handle.binding();
        let bytes = client.read_one(binding);
        let output_values = O::from_bytes(&bytes);

        assert_approx_equal(output_values, &expected_values);
    }

    fn cpu_sum<F: Float>(&self, values: &[F]) -> Vec<F> {
        let mut expected = vec![F::new(0.0); self.num_output_values()];
        #[allow(clippy::needless_range_loop)]
        for input_index in 0..values.len() {
            let output_index = self.to_output_index(input_index);
            expected[output_index] += values[input_index];
        }
        expected
    }

    fn cpu_prod<F: Float>(&self, values: &[F]) -> Vec<F> {
        let mut expected = vec![F::new(1.0); self.num_output_values()];
        #[allow(clippy::needless_range_loop)]
        for value_index in 0..values.len() {
            let output_index = self.to_output_index(value_index);
            expected[output_index] *= values[value_index];
        }
        expected
    }

    fn cpu_mean<F: Float>(&self, values: &[F]) -> Vec<F> {
        self.cpu_sum(values)
            .into_iter()
            .map(|sum| sum / F::new(self.shape[self.reduce_dim as usize] as f32))
            .collect()
    }

    fn cpu_argmax<F: Float>(&self, values: &[F]) -> Vec<u32> {
        let mut expected = vec![(F::MIN, 0_u32); self.num_output_values()];
        #[allow(clippy::needless_range_loop)]
        for input_index in 0..values.len() {
            let output_index = self.to_output_index(input_index);
            let (best, _) = expected[output_index];
            let candidate = values[input_index];
            if candidate > best {
                let coordinate = self.to_input_coordinate(input_index / self.line_size as usize);
                expected[output_index] = (candidate, coordinate[self.reduce_dim as usize] as u32);
            }
        }
        expected.into_iter().map(|(_, i)| i).collect()
    }

    fn cpu_argmin<F: Float>(&self, values: &[F]) -> Vec<u32> {
        let mut expected = vec![(F::MAX, 0_u32); self.num_output_values()];
        #[allow(clippy::needless_range_loop)]
        for input_index in 0..values.len() {
            let output_index = self.to_output_index(input_index);
            let (best, _) = expected[output_index];
            let candidate = values[input_index];
            if candidate < best {
                let coordinate = self.to_input_coordinate(input_index / self.line_size as usize);
                expected[output_index] = (candidate, coordinate[self.reduce_dim as usize] as u32);
            }
        }
        expected.into_iter().map(|(_, i)| i).collect()
    }

    fn num_output_values(&self) -> usize {
        self.line_size as usize * self.shape.iter().product::<usize>()
            / self.shape[self.reduce_dim as usize]
    }

    fn to_output_index(&self, input_index: usize) -> usize {
        let line_size = self.line_size as usize;
        let mut coordinate = self.to_input_coordinate(input_index / line_size);
        coordinate[self.reduce_dim as usize] = 0;
        self.from_output_coordinate(coordinate) * line_size + input_index % line_size
    }

    fn to_input_coordinate(&self, index: usize) -> Vec<usize> {
        self.stride
            .iter()
            .zip(self.shape.iter())
            .map(|(stride, shape)| (index / stride) % shape)
            .collect()
    }

    #[allow(clippy::wrong_self_convention)]
    fn from_output_coordinate(&self, coordinate: Vec<usize>) -> usize {
        coordinate
            .into_iter()
            .zip(self.output_stride().iter())
            .map(|(c, s)| c * s)
            .sum()
    }

    fn output_stride(&self) -> Vec<usize> {
        let stride = self.stride[self.reduce_dim as usize];
        let shape = self.shape[self.reduce_dim as usize];
        self.stride
            .iter()
            .map(|s| match s.cmp(&stride) {
                std::cmp::Ordering::Equal => 1,
                std::cmp::Ordering::Greater => s / shape,
                std::cmp::Ordering::Less => *s,
            })
            .collect()
    }

    fn random_input_values<F: Float>(&self) -> Vec<F> {
        let size = self.shape.iter().product::<usize>() * self.line_size as usize;
        let rng = StdRng::seed_from_u64(self.pseudo_random_seed());
        let distribution = Uniform::new_inclusive(-PRECISION, PRECISION);
        let factor = 1.0 / (PRECISION as f32);
        distribution
            .sample_iter(rng)
            .take(size)
            .map(|r| F::new(r as f32 * factor))
            .collect()
    }

    // We don't need a fancy crypto-secure seed as this is only for testing.
    fn pseudo_random_seed(&self) -> u64 {
        (self.stride.len() * self.shape[0]) as u64 ^ self.cube_dim.num_elems() as u64
    }
}

pub fn assert_approx_equal<N: Numeric>(actual: &[N], expected: &[N]) {
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        let a = a.to_f32().unwrap();
        let e = e.to_f32().unwrap();
        let diff = (a - e).abs();
        if e == 0.0 {
            assert!(
                diff < 1e-10,
                "Values are not approx equal: index={} actual={}, expected={}, difference={}",
                i,
                a,
                e,
                diff,
            );
        } else {
            let rel_diff = diff / e.abs();
            assert!(
                rel_diff < 0.0625,
                "Values are not approx equal: index={} actual={}, expected={}",
                i,
                a,
                e
            );
        }
    }
}
