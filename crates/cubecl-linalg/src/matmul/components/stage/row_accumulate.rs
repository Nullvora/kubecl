use std::marker::PhantomData;

use cubecl_core as cubecl;
use cubecl_core::prelude::*;

use crate::matmul::components::stage::base::Matmul as _;
use crate::matmul::{
    components::{
        config::MatmulConfig,
        global,
        stage::{self, Config as _, StageReader, StageWriter},
        tile, Ident, MatmulKernel, MatmulProblem, MatrixLayout, PlaneMapper, StageDim,
    },
    kernels::matmul::{create_stage_dim, AdvancedConfig},
};

use super::reader::{LhsReader, RhsReader};
use super::tiling_order::TilingOrderConfig;
use super::StageSize;

/// Performs matrix multiplication at the stage level, where each plane is responsible for a row of tiles:
/// - One plane per tile in m dimension,
/// - One accumulator per tile in n dimension
///
/// # Assumptions
/// - There are as many planes as the stage size in m
pub struct Matmul<I: Numeric, O: Numeric, Acc: Numeric, TMM: tile::Matmul<I, Acc>, SS: StageSize> {
    _input_precision: PhantomData<I>,
    _output_precision: PhantomData<O>,
    _accumulator_precision: PhantomData<Acc>,
    _instruction: PhantomData<TMM>,
    _block_size: PhantomData<SS>,
}

#[cube]
impl<I, O, Acc, TMM, SS> stage::Matmul<I, O, LhsReader<I>, RhsReader<I>>
    for Matmul<I, O, Acc, TMM, SS>
where
    I: Numeric,
    O: Numeric,
    Acc: Numeric,
    TMM: tile::Matmul<I, Acc>,
    SS: StageSize,
{
    const M: u32 = SS::NUM_M * TMM::M;
    const N: u32 = SS::NUM_N * TMM::N;
    const K: u32 = SS::NUM_K * TMM::K;
    type Accumulator = Sequence<TMM::Out>;

    fn execute(
        lhs: &LhsReader<I>,
        rhs: &RhsReader<I>,
        acc: &mut Self::Accumulator,
        #[comptime] config: Self::Config,
    ) {
        let mut instruction_lhs = TMM::init_lhs(config.to_tmm_config());
        let mut instruction_rhs = TMM::init_rhs(config.to_tmm_config());

        #[unroll]
        for buffer_iter in 0..SS::NUM_K {
            let tile_lhs = LhsReader::read_tile::<Self::Config>(
                lhs,
                Self::plane_id(),
                buffer_iter,
                0u32,
                config,
            );
            TMM::fill_lhs(tile_lhs, &mut instruction_lhs, config.to_tmm_config());

            #[unroll]
            for accumulator_iter in 0..acc.len() {
                let tile_rhs = RhsReader::read_tile::<Self::Config>(
                    rhs,
                    Self::plane_id(),
                    buffer_iter,
                    accumulator_iter,
                    config,
                );
                TMM::fill_rhs(tile_rhs, &mut instruction_rhs, config.to_tmm_config());

                let accumulator = acc.index_mut(accumulator_iter);
                TMM::execute(
                    &instruction_lhs,
                    &instruction_rhs,
                    accumulator,
                    config.to_tmm_config(),
                );
            }
        }
    }

    fn acc_init_zeros(#[comptime] config: Self::Config) -> Self::Accumulator {
        let mut accumulators = Sequence::<TMM::Out>::new();

        #[unroll]
        for _ in 0..SS::NUM_N {
            accumulators.push(TMM::init_output(config.to_tmm_config()));
        }

        accumulators
    }

    fn acc_read<SW: StageWriter<O>, G: global::Config>(
        acc: &Self::Accumulator,
        out: &mut SW,
        #[comptime] stage_config: Self::Config,
        #[comptime] global_config: G,
    ) {
        let out_smem_line_size = global_config.stage_line_size(Ident::Out);
        let num_tile_lines =
            stage_config.stage_dim(Ident::Out).tile_num_elements() / out_smem_line_size;

        let start = num_tile_lines * Self::plane_id();
        let mut out_smem = SharedMemory::<Acc>::new_lined(
            num_tile_lines * stage_config.num_planes(),
            out_smem_line_size,
        );

        #[unroll]
        for accumulator_iter in 0..acc.len() {
            let accumulator = acc.index(accumulator_iter);
            let smem_slice = out_smem.slice_mut(start, start + num_tile_lines);
            TMM::read_output(accumulator, smem_slice, stage_config.to_tmm_config());
            SW::write::<Acc, G>(
                out,
                smem_slice.as_slice(),
                Self::plane_id(),
                accumulator_iter,
                global_config,
            );
        }
    }
}

impl<I, O, Acc, TMM, SS> MatmulKernel<I, O> for Matmul<I, O, Acc, TMM, SS>
where
    I: Numeric,
    O: Numeric,
    Acc: Numeric,
    TMM: tile::Matmul<I, Acc>,
    SS: StageSize,
{
    type Config = Config<TMM::Config>;

    fn check_config(config: Self::Config) {
        comptime!(check_num_planes(
            config.stage_dim(Ident::Lhs).num_tiles_x,
            config.num_planes()
        ));
        TMM::check_config(config.to_tmm_config());
    }

    fn check_availability<R: Runtime>(
        client: &ComputeClient<R::Server, R::Channel>,
    ) -> Result<(), &str> {
        TMM::check_availability::<R>(client)
    }

    fn make_config(
        problem: &MatmulProblem,
        cube_dim: &CubeDim,
        cube_count: &CubeCount,
        advanced_config: &AdvancedConfig,
    ) -> Self::Config {
        let tmm_config = TMM::make_config(problem, cube_dim, cube_count, advanced_config);

        let (stage_m, stage_n, stage_k) = (Self::M, Self::N, Self::K);
        let (tile_m, tile_n, tile_k) = (TMM::M, TMM::N, TMM::K);
        let (lhs_stage_dim, rhs_stage_dim, out_stage_dim) =
            create_stage_dim(stage_m, stage_n, stage_k, tile_m, tile_n, tile_k);

        Config::new(
            tmm_config,
            lhs_stage_dim,
            rhs_stage_dim,
            out_stage_dim,
            cube_dim.y,
            advanced_config.tiling_order,
        )
    }
}

#[cube]
impl<I, O, Acc, Tmm, SS> PlaneMapper for Matmul<I, O, Acc, Tmm, SS>
where
    I: Numeric,
    O: Numeric,
    Acc: Numeric,
    Tmm: tile::Matmul<I, Acc>,
    SS: StageSize,
{
    fn plane_id() -> u32 {
        UNIT_POS_Y
    }

    fn plane_unit() -> u32 {
        UNIT_POS_X
    }
}

fn check_num_planes(expected_num_planes: u32, actual_num_planes: u32) {
    assert_eq!(
        expected_num_planes, actual_num_planes,
        "Error: Expected {expected_num_planes} planes, but found {actual_num_planes}. 
        The number of planes is equal to cube dimension y which should be set to {expected_num_planes}.",
    );
}

#[derive(CubeType, Copy, Clone, Debug, Hash, PartialEq, Eq)]
/// Configuration for the row accumulate matmul
pub struct Config<T: tile::Config> {
    tmm_config: T,
    lhs_stage_dim: StageDim,
    rhs_stage_dim: StageDim,
    out_stage_dim: StageDim,
    num_planes: u32,
    tiling_order: TilingOrderConfig,
}

impl<T: tile::Config> stage::Config for Config<T> {
    type TmmConfig = T;

    fn to_tmm_config(self) -> Self::TmmConfig {
        self.tmm_config
    }

    fn line_size(&self, ident: Ident) -> u32 {
        self.tmm_config.line_size(ident)
    }

    fn stage_dim(&self, ident: Ident) -> StageDim {
        match ident {
            Ident::Lhs => self.lhs_stage_dim,
            Ident::Rhs => self.rhs_stage_dim,
            Ident::Out => self.out_stage_dim,
        }
    }

    fn layout(&self, ident: Ident) -> MatrixLayout {
        self.tmm_config.layout(ident)
    }

    fn num_planes(&self) -> u32 {
        self.num_planes
    }

    fn plane_dim(&self) -> u32 {
        self.tmm_config.plane_dim()
    }

    fn tiling_order(&self) -> TilingOrderConfig {
        self.tiling_order
    }
}

impl<T: tile::Config> MatmulConfig for Config<T> {}

impl<T: tile::Config> Config<T> {
    pub fn new(
        tmm_config: T,
        lhs_stage_dim: StageDim,
        rhs_stage_dim: StageDim,
        out_stage_dim: StageDim,
        num_planes: u32,
        tiling_order: TilingOrderConfig,
    ) -> Self {
        Self {
            tmm_config,
            lhs_stage_dim,
            rhs_stage_dim,
            out_stage_dim,
            num_planes,
            tiling_order,
        }
    }
}
