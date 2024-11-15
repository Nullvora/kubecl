use cubecl_core as cubecl;
use cubecl_core::prelude::*;

use crate::matmul::components::{config::MatmulConfig, Ident, MatmulKernel, MatrixLayout};

#[cube]
/// Provides matrix multiplication operations at the tile level.
///
/// At the tile level,
///  - Inputs are raw slices of data, called tiles.
///  - units within one plane can collaborate to solve the problem
///  - dimensions M, N and K are fixed to an integer, and the
///    matrix multiplication works only for size (M, K) · (K, N) = (M, N).
///
/// Assumptions:
///  - Slices given as inputs must always be valid. If the actual matrix multiplication
///    should be done on smaller sizes than M, N and K, padding with zeros must be done beforehand.
///  - Enough units are present to perform the whole computation
pub trait Matmul<I: Numeric, O: Numeric>:
    'static + Send + Sync + MatmulKernel<I, O, Config: Config>
{
    /// Number of rows of LHS
    const M: u32;
    /// Number of columns of RHS
    const N: u32;
    /// Common dimension of LHS and RHS
    const K: u32;

    /// Contains LHS data that can be split across the units
    type Lhs: CubeType;
    /// Contains RHS data that can be split across the units
    type Rhs: CubeType;
    /// Contains output data that can be split across the units
    type Accumulator: CubeType;

    /// Executes the matrix multiplication of LHS and RHS, adding the result to the output
    fn execute(
        lhs: &Self::Lhs,
        rhs: &Self::Rhs,
        out: &mut Self::Accumulator,
        #[comptime] config: Self::Config,
    );

    /// Create the container for LHS data
    ///
    /// # Safety
    ///
    /// This may point towards uninitialized memory.
    /// Make sure to call fill_lhs prior to execute.
    fn init_lhs(#[comptime] config: Self::Config) -> Self::Lhs;

    /// Create the container for RHS data
    ///
    /// # Safety
    ///
    /// This may point towards uninitialized memory.
    /// Make sure to call fill_rhs prior to execute.
    fn init_rhs(#[comptime] config: Self::Config) -> Self::Rhs;

    /// Fill the container of LHS with data
    fn fill_lhs(slice: &Slice<Line<I>>, lhs: &mut Self::Lhs, #[comptime] config: Self::Config);

    /// Fill the container of RHS with data
    fn fill_rhs(slice: &Slice<Line<I>>, rhs: &mut Self::Rhs, #[comptime] config: Self::Config);

    /// Write the content of the output container to the given slice
    fn read_accumulator<C: Numeric>(
        out: &Self::Accumulator,
        slice: &mut SliceMut<Line<C>>,
        #[comptime] config: Self::Config,
    );

    /// Create the container to receive the execution output.
    ///
    /// # Safety
    ///
    /// The output container must be initialized to some value (typically 0),
    /// because the execution adds to the already present value.
    fn init_accumulator(#[comptime] config: Self::Config) -> Self::Accumulator;

    /// Set the accumulator to zeros
    fn zero_accumulator(acc: &mut Self::Accumulator, #[comptime] config: Self::Config);
}

/// Configuration for the Tile matmul (TMM) level
pub trait Config: MatmulConfig {
    /// Returns the size of the plane dimension
    fn plane_dim(&self) -> u32;

    /// Returns the [MatrixLayout] for the given ident
    fn layout(&self, ident: Ident) -> MatrixLayout;

    /// Returns the line size for the given ident
    fn line_size(&self, ident: Ident) -> u32;
}
