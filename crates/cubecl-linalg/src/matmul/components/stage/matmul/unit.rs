use crate::matmul::components::global::UnitWriter;
use crate::matmul::components::stage::ReaderFamily;
use crate::matmul::components::stage::StageBuffering;
use crate::matmul::components::stage::shared::CommonStageConfig;
use crate::matmul::components::stage::{StageConfig, StageMatmulFamily, TilingLayout};
use crate::matmul::components::tile::TileMatmulConfigInput;
use crate::matmul::components::tile::TileMatmulFamily;
use crate::matmul::components::{
    CompleteStageTiling, InvalidConfigError, MatmulConfigFactory, MatmulLineSizes, MatmulPrecision,
    MatmulSize,
};
use crate::matmul::components::{Ident, MatmulProblem};
use crate::matmul::kernels::MatmulAvailabilityError;
use crate::matmul::kernels::matmul::StageInput;
use core::marker::PhantomData;
use cubecl::prelude::*;
use cubecl_core as cubecl;
use cubecl_std::tensor::r#virtual::{ReadWrite, VirtualTensor};

use super::partitioned_stage_matmul::{PartitionedStageMatmul, StagePartitioner};

type UnitMatmul<MP, TMM, RL, RR> = PartitionedStageMatmul<MP, TMM, RL, RR, UnitPartitioner>;

pub struct UnitPartitioner {}

#[cube]
impl StagePartitioner for UnitPartitioner {
    type Writer<EO: Numeric> = UnitWriter<EO>;

    fn init_writer<EO: Numeric>(
        tensor: VirtualTensor<EO, ReadWrite>,
        x_offset: u32,
        y_offset: u32,
        batch_offset: u32,
    ) -> Self::Writer<EO> {
        UnitWriter::<EO>::new(tensor, x_offset, y_offset, batch_offset)
    }

    fn position() -> u32 {
        UNIT_POS
    }

    fn num_primitives<S: StageConfig>(#[comptime] config: S) -> comptime_type!(u32) {
        config.num_planes() * config.plane_dim()
    }
}

pub struct UnitMatmulFamily<TMM: TileMatmulFamily, RF: ReaderFamily> {
    _phantom: PhantomData<(TMM, RF)>,
}

impl<TMM: TileMatmulFamily, RF: ReaderFamily> StageMatmulFamily for UnitMatmulFamily<TMM, RF> {
    fn stage_shape(config: &Self::Config) -> MatmulSize {
        config.tiling.total_shape()
    }

    fn tile_count(config: &Self::Config) -> MatmulSize {
        config.tiling.tile_count
    }

    fn tile_shape(config: &Self::Config) -> MatmulSize {
        config.tiling.tile_shape
    }

    type LhsReader = RF;
    type RhsReader = RF;
    type Matmul<MP: MatmulPrecision, TL: TilingLayout, TR: TilingLayout> =
        UnitMatmul<MP, TMM::Matmul<MP>, RF::Reader<MP::ES, TL>, RF::Reader<MP::ES, TR>>;
}

impl<TMM: TileMatmulFamily, RF: ReaderFamily> MatmulConfigFactory for UnitMatmulFamily<TMM, RF> {
    type Input = StageInput;
    type Config = CommonStageConfig<TMM::Config>;

    fn check_config(config: &Self::Config) -> Result<(), InvalidConfigError> {
        let num_acc = config.tiling_dimensions(Ident::Out).tile_count();
        let acc_per_unit = config.accumulator_count().num_tiles();

        if num_acc % acc_per_unit != 0 {
            return Err(Box::new(format!(
                "Error: Number of accumulators {num_acc} should be divisible by number of accumulators per unit {acc_per_unit}."
            )));
        }

        let num_units_needed = num_acc / acc_per_unit;
        let num_units = config.plane_dim() * config.num_planes();

        if num_units != num_units_needed {
            return Err(Box::new(format!(
                "Error: Number of units {num_units} should be {num_units_needed}."
            )));
        }

        if config.buffering() == StageBuffering::Double && config.accumulator_count().n < 2 {
            return Err(Box::new(format!(
                "Error: Tried doing double buffering with only one tile to compute."
            )));
        }

        TMM::check_config(&config.to_tmm_config())
    }

    fn check_availability<R: Runtime, MP: MatmulPrecision>(
        client: &ComputeClient<R::Server, R::Channel>,
        config: &Self::Config,
    ) -> Result<(), MatmulAvailabilityError> {
        TMM::check_availability::<R, MP>(client, &config.tmm_config)
    }

    fn make_config(
        stage_input: Self::Input,
        problem: &MatmulProblem,
        line_sizes: &MatmulLineSizes,
        cube_dim: &CubeDim,
        cube_count: &CubeCount,
        quantized: bool,
    ) -> Self::Config {
        let tile_shape = stage_input.tiling.tile_shape;
        let tile_count = stage_input.tiling.tile_count;

        let tile_input = TileMatmulConfigInput {
            vectorization: stage_input.stage_vectorization,
            size: tile_shape,
        };
        let tmm_config = TMM::make_config(
            tile_input, problem, line_sizes, cube_dim, cube_count, quantized,
        );

        let tiling = CompleteStageTiling {
            tile_shape,
            tile_count,
        };

        CommonStageConfig::new(
            tmm_config,
            tiling,
            cube_dim.y,
            quantized,
            stage_input.stage_buffering,
            stage_input.num_stages,
            stage_input.accumulator_count,
        )
    }
}
