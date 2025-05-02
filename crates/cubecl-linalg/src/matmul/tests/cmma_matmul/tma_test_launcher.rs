use std::marker::PhantomData;

use cubecl_core::CubeElement;
use cubecl_core::prelude::*;
use cubecl_core::tensor_line_size_parallel;

use crate::matmul::components::Ident;
use crate::matmul::components::MatmulProblem;
use crate::matmul::components::MatmulSelection;
use crate::matmul::components::MatrixLayout;
use crate::matmul::components::{MatmulConfigFactory, global::args::TensorMapArgs};
use crate::matmul::components::{
    MatmulLaunch,
    global::args::{ConcreteInputsFactory, TensorMapInputs},
};
use crate::matmul::kernels::matmul::Algorithm;
use crate::matmul::tests::test_utils::Sample;
use crate::matmul::tests::test_utils::TestPrecision;

use super::matmul_test_launcher::{TensorRawParts, shape, tensor_size, transpose};

/// Test the correctness of the specified Matmul on the given device,
/// against a naive CPU implementation over the given problem
pub fn test_tma_matmul_algorithm<A, P, R>(
    client: ComputeClient<R::Server, R::Channel>,
    mut problem: MatmulProblem,
    input: <A::BatchMatmul as MatmulConfigFactory>::Input,
    selection: MatmulSelection,
) where
    A: Algorithm,
    P: TestPrecision,
    R: Runtime,
{
    let env = std::env::var("MATMUL_TEST_MODE");

    let panic_on_launch_err = match env {
        Ok(val) => match val.as_str() {
            "panic" => true,
            "skip" => false,
            _ => false,
        },
        Err(_) => false,
    };
    let lhs = tensor_raw_parts::<P, R>(&client, &problem, Ident::Lhs);
    let rhs = tensor_raw_parts::<P, R>(&client, &problem, Ident::Rhs);
    let out = tensor_raw_parts::<P, R>(&client, &problem, Ident::Out);

    // No point vectorizing when we never deal with individual values anyways
    problem.lhs_line_size = 1;
    problem.rhs_line_size = 1;
    problem.out_line_size = tensor_line_size_parallel(
        R::line_size_elem(&P::EG::as_elem_native_unchecked()),
        &out.shape,
        &out.strides,
        out.strides.len() - 1,
    );

    let cube_dim = A::cube_dim(&selection);
    let cube_count = A::cube_count(&selection, &problem);

    let config = match A::make_config(input, &problem, &cube_dim, &cube_count, P::QUANTIZED) {
        Ok(config) => config,
        Err(err) => {
            let msg = format!("Can't launch the test: {:?}", err);
            if panic_on_launch_err {
                panic!("{msg}");
            } else {
                println!("{msg}");
                return;
            }
        }
    };

    if let Err(err) = A::check_availability::<R, (P::EG, P::ES, f32, P::EG)>(&client, &config) {
        let msg = format!("Skipped - not supported: {:?}", err);
        if panic_on_launch_err {
            panic!("{msg}")
        } else {
            println!("{msg}");
            client.flush();
            return;
        }
    }

    let elem_size = size_of::<P::EG>();
    let lhs_handle = TensorHandleRef {
        handle: &lhs.handle,
        strides: &lhs.strides,
        shape: &lhs.shape,
        elem_size,
        runtime: PhantomData,
    };
    let rhs_handle = TensorHandleRef {
        handle: &rhs.handle,
        strides: &rhs.strides,
        shape: &rhs.shape,
        elem_size,
        runtime: PhantomData,
    };

    let inputs =
        TensorMapInputs::create(&lhs_handle, &None, &rhs_handle, &None, &selection, &problem);
    let output = unsafe {
        TensorArg::<R>::from_raw_parts::<P::EG>(
            &out.handle,
            &out.strides,
            &out.shape,
            problem.out_line_size,
        )
    };

    unsafe {
        A::BatchMatmul::launch_unchecked::<((P::EG, P::ES, P::EA, P::EG), TensorMapArgs), R>(
            &client,
            cube_dim,
            cube_count,
            inputs,
            output,
            ScalarArg::new(problem.k as u32),
            config,
        );
    }

    P::assert_result::<R>(
        &lhs.original_data.unwrap(),
        lhs.quant_params,
        &rhs.original_data.unwrap(),
        rhs.quant_params,
        &problem,
        &client,
        out.handle,
        out.quant_params,
        &out.shape,
        &out.strides,
    );
}

fn tensor_raw_parts<P: TestPrecision, R: Runtime>(
    client: &ComputeClient<R::Server, R::Channel>,
    problem: &MatmulProblem,
    ident: Ident,
) -> TensorRawParts<P::EG> {
    match ident {
        Ident::Lhs => {
            let original_data = P::EG::sample(tensor_size(problem, Ident::Lhs), 1234);

            let mut shape = shape(problem, ident);
            let rank = shape.len();

            let data = match problem.lhs_layout {
                MatrixLayout::RowMajor => original_data.clone(),
                MatrixLayout::ColMajor => {
                    shape.swap(rank - 1, rank - 2);
                    transpose::<P::EG>(&original_data, problem.num_batches(), problem.m, problem.k)
                }
            };

            let (handle, mut strides) =
                client.create_tensor(P::EG::as_bytes(&data), &shape, size_of::<P::EG>());

            if matches!(problem.lhs_layout, MatrixLayout::ColMajor) {
                shape.swap(rank - 1, rank - 2);
                strides.swap(rank - 1, rank - 2);
            }

            TensorRawParts {
                handle,
                scale: None,
                shape,
                strides,
                original_data: Some(original_data),
                quant_params: None,
            }
        }
        Ident::Rhs => {
            let original_data = P::EG::sample(tensor_size(problem, Ident::Rhs), 5678);

            let mut shape = shape(problem, ident);
            let rank = shape.len();

            let data = match problem.rhs_layout {
                MatrixLayout::RowMajor => original_data.clone(),
                MatrixLayout::ColMajor => {
                    shape.swap(rank - 1, rank - 2);
                    transpose::<P::EG>(&original_data, problem.num_batches(), problem.k, problem.n)
                }
            };

            let (handle, mut strides) =
                client.create_tensor(P::EG::as_bytes(&data), &shape, size_of::<P::EG>());

            if matches!(problem.rhs_layout, MatrixLayout::ColMajor) {
                shape.swap(rank - 1, rank - 2);
                strides.swap(rank - 1, rank - 2);
            }

            TensorRawParts {
                handle,
                scale: None,
                shape,
                strides,
                original_data: Some(original_data),
                quant_params: None,
            }
        }
        Ident::Out => {
            let zero = P::EG::from_int(0);

            let data = vec![zero; tensor_size(problem, Ident::Out)];

            let shape = shape(problem, Ident::Out);
            let (handle, strides) =
                client.create_tensor(P::EG::as_bytes(&data), &shape, size_of::<P::EG>());
            TensorRawParts {
                handle,
                scale: None,
                shape,
                strides,
                original_data: None,
                quant_params: None,
            }
        }
    }
}
