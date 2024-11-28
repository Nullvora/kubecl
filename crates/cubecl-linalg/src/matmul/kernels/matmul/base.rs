use cubecl_core::prelude::*;

use cubecl_core::{
    client::ComputeClient,
    frontend::{TensorArg, TensorHandleRef},
    tensor_line_size, Runtime,
};

use crate::matmul;
use crate::matmul::components::{MatmulLaunch, MatmulProblem};
use crate::matmul::kernels::MatmulLaunchError;
use crate::tensor::{into_contiguous, matrix_layout, MatrixLayout, TensorHandle};

use super::algorithm::{CmmaSelector, PlaneMmaSelector};
use super::config::AdvancedConfig;
use super::Algorithm;

/// Launch a matrix multiplication kernel.
///
/// Cmma will be used if enabled
/// Will fail if unavailable
pub fn launch<R: Runtime, EG: Numeric>(
    client: &ComputeClient<R::Server, R::Channel>,
    lhs: TensorHandle<R, EG>,
    rhs: TensorHandle<R, EG>,
    out: TensorHandle<R, EG>,
    disable_cmma: bool,
) -> Result<TensorHandle<R, EG>, MatmulLaunchError> {
    let result = launch_ref::<R, EG>(
        client,
        &lhs.as_ref(),
        &rhs.as_ref(),
        &out.as_ref(),
        disable_cmma,
    );

    match result {
        Ok(_) => Ok(out),
        Err(e) => Err(e),
    }
}

/// Launch a matrix multiplication kernel.
///
/// Cmma will be used if available and enabled,
/// otherwise it will fall back on a non-cmma implementation
pub fn launch_ref<R: Runtime, EG: Numeric>(
    client: &ComputeClient<R::Server, R::Channel>,
    lhs: &TensorHandleRef<'_, R>,
    rhs: &TensorHandleRef<'_, R>,
    out: &TensorHandleRef<'_, R>,
    disable_cmma: bool,
) -> Result<(), MatmulLaunchError> {
    let check_layout = |tensor: &TensorHandleRef<'_, R>| match matrix_layout(tensor.strides) {
        MatrixLayout::Contiguous => (false, false),
        MatrixLayout::MildlyPermuted {
            transposed,
            batch_swap: _,
        } => (false, transposed),
        MatrixLayout::HighlyPermuted => (true, false),
    };

    let (lhs_make_contiguous, lhs_transposed) = check_layout(lhs);
    let (rhs_make_contiguous, rhs_transposed) = check_layout(rhs);

    match (lhs_make_contiguous, rhs_make_contiguous) {
        (false, false) => matmul_cmma_ref_no_check::<R, EG>(
            client,
            lhs,
            rhs,
            out,
            (lhs_transposed, rhs_transposed),
            disable_cmma,
        ),
        (false, true) => matmul_cmma_ref_no_check::<R, EG>(
            client,
            lhs,
            &into_contiguous::<R, EG>(client, rhs).as_ref(),
            out,
            (lhs_transposed, rhs_transposed),
            disable_cmma,
        ),
        (true, false) => matmul_cmma_ref_no_check::<R, EG>(
            client,
            &into_contiguous::<R, EG>(client, lhs).as_ref(),
            rhs,
            out,
            (lhs_transposed, rhs_transposed),
            disable_cmma,
        ),
        (true, true) => matmul_cmma_ref_no_check::<R, EG>(
            client,
            &into_contiguous::<R, EG>(client, lhs).as_ref(),
            &into_contiguous::<R, EG>(client, rhs).as_ref(),
            out,
            (lhs_transposed, rhs_transposed),
            disable_cmma,
        ),
    }
}

fn matmul_cmma_ref_no_check<R: Runtime, EG: Numeric>(
    client: &ComputeClient<R::Server, R::Channel>,
    lhs: &TensorHandleRef<'_, R>,
    rhs: &TensorHandleRef<'_, R>,
    out: &TensorHandleRef<'_, R>,
    transposed: (bool, bool),
    disable_cmma: bool,
) -> Result<(), MatmulLaunchError> {
    let rank = lhs.strides.len();

    let m = lhs.shape[rank - 2] as u32;
    let k = lhs.shape[rank - 1] as u32;
    let n = rhs.shape[rank - 1] as u32;

    let available_vectorizations = R::supported_line_sizes();
    let lhs_line_size =
        tensor_line_size(available_vectorizations, lhs.shape, lhs.strides, rank - 1);
    let rhs_line_size =
        tensor_line_size(available_vectorizations, rhs.shape, rhs.strides, rank - 1);
    let out_line_size =
        tensor_line_size(available_vectorizations, out.shape, out.strides, rank - 1);

    let problem = MatmulProblem {
        m: m as usize,
        n: n as usize,
        k: k as usize,
        batches: (
            lhs.shape[..lhs.shape.len() - 2].to_vec(),
            rhs.shape[..rhs.shape.len() - 2].to_vec(),
        ),
        lhs_layout: match transposed.0 {
            true => matmul::components::MatrixLayout::ColMajor,
            false => matmul::components::MatrixLayout::RowMajor,
        },
        rhs_layout: match transposed.1 {
            true => matmul::components::MatrixLayout::ColMajor,
            false => matmul::components::MatrixLayout::RowMajor,
        },
        lhs_line_size,
        rhs_line_size,
        out_line_size,
    };

    matmul_select_kernel::<R, EG>(client, lhs, rhs, out, problem, disable_cmma)
}

fn matmul_select_kernel<R: Runtime, EG: Numeric>(
    client: &ComputeClient<R::Server, R::Channel>,
    lhs: &TensorHandleRef<'_, R>,
    rhs: &TensorHandleRef<'_, R>,
    out: &TensorHandleRef<'_, R>,
    problem: MatmulProblem,
    disable_cmma: bool,
) -> Result<(), MatmulLaunchError> {
    if disable_cmma {
        PlaneMmaSelector::select_kernel::<R, EG>(client, lhs, rhs, out, problem)
    } else {
        CmmaSelector::select_kernel::<R, EG>(client, lhs, rhs, out, problem)
    }
}

pub(crate) fn matmul_cube_preparation<R: Runtime, EG: Numeric, D: Algorithm<EG>>(
    client: &ComputeClient<R::Server, R::Channel>,
    lhs: &TensorHandleRef<'_, R>,
    rhs: &TensorHandleRef<'_, R>,
    out: &TensorHandleRef<'_, R>,
    problem: MatmulProblem,
) -> Result<(), MatmulLaunchError> {
    D::check_availability::<R>(client)?;

    let cube_dim = D::cube_dim();
    let cube_count = D::cube_count(&problem);
    let advanced_config = D::advanced_config();

    launch_matmul::<R, EG, D>(
        client,
        lhs,
        rhs,
        out,
        problem,
        cube_dim,
        cube_count,
        advanced_config,
    )
}

#[allow(clippy::too_many_arguments)]
fn launch_matmul<R: Runtime, EG: Numeric, D: Algorithm<EG>>(
    client: &ComputeClient<R::Server, R::Channel>,
    lhs: &TensorHandleRef<'_, R>,
    rhs: &TensorHandleRef<'_, R>,
    out: &TensorHandleRef<'_, R>,
    problem: MatmulProblem,
    cube_dim: CubeDim,
    cube_count: CubeCount,
    advanced_config: AdvancedConfig,
) -> Result<(), MatmulLaunchError> {
    let config = D::make_config(&problem, &cube_dim, &cube_count, &advanced_config)?;

    unsafe {
        D::BatchMatmul::launch_unchecked::<R>(
            client,
            cube_dim,
            cube_count,
            TensorArg::<R>::from_raw_parts::<D::EG>(
                lhs.handle,
                lhs.strides,
                lhs.shape,
                problem.lhs_line_size,
            ),
            TensorArg::<R>::from_raw_parts::<D::EG>(
                rhs.handle,
                rhs.strides,
                rhs.shape,
                problem.rhs_line_size,
            ),
            TensorArg::<R>::from_raw_parts::<D::EG>(
                out.handle,
                out.strides,
                out.shape,
                problem.out_line_size,
            ),
            config,
        );
    };

    Ok(())
}
