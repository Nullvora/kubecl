use std::marker::PhantomData;

use crate::matmul::components::global::Quantization;
use crate::matmul::components::global::load::SyncBufferLoadingStrategy;
use crate::matmul::components::{
    FormattedConfigError, Ident, InputIdent, InvalidConfigError, MatmulPrecision,
};
use crate::matmul::components::{
    global::{GlobalConfig, LoadingValidation, tensor_view::TensorReader},
    stage::{ContiguousTilingLayout, StageMemory, TilingOrder},
};
use cubecl_core as cubecl;
use cubecl_core::prelude::*;
use cubecl_std::{CubeOption, CubeOptionExpand};

use super::LoadingJob;

#[derive(CubeType, Clone, Copy)]
/// Each tile is guaranteed to be loaded entirely by the same plane.
/// Each plane can load multiple tiles, provided the number of planes evenly divides the number of tiles.
/// In this case, a plane loads contiguous tiles following the `TilingOrder`,
/// until it would otherwise write to the opposite buffer. At that point, it continues on the next
/// row or column of the same buffer, skipping over the memory region of the other buffer.
pub struct LoadingStrategy<T: TilingOrder> {
    #[cube(comptime)]
    tiling_order: PhantomData<T>,
}

impl<T: TilingOrder> LoadingValidation for LoadingStrategy<T> {
    fn check<C: GlobalConfig>(config: &C, ident: Ident) -> Result<(), InvalidConfigError> {
        let tiling = config.tiling_dimensions(ident);
        let line_size = config.global_line_size(ident);

        let num_planes = config.num_planes();
        let num_tiles = tiling.tile_count();

        if num_tiles % num_planes != 0 {
            return Err(FormattedConfigError::new(move || {
                format!(
                    "Number of planes {:?} must divide number of tiles {:?} for tilewise loading.",
                    num_planes, num_tiles,
                )
            }));
        }

        let num_tiles_per_plane = comptime!(num_tiles / num_planes);
        let num_lines_per_tile = comptime!(tiling.tile_size() / line_size);
        let num_lines_per_plane = num_lines_per_tile * num_tiles_per_plane;
        let num_planes = config.plane_dim();

        if num_lines_per_plane % num_planes != 0 {
            return Err(FormattedConfigError::new(move || {
                format!(
                    "Number of planes {:?} must divide number of lines per plane {:?} for tilewise loading.",
                    num_planes, num_lines_per_plane,
                )
            }));
        }

        Ok(())
    }
}

#[cube]
impl<TO: TilingOrder> SyncBufferLoadingStrategy for LoadingStrategy<TO> {
    type TilingLayout = ContiguousTilingLayout<TO>;
    type Job<MP: MatmulPrecision> = Job;

    fn new_job<MP: MatmulPrecision, G: GlobalConfig>(
        #[comptime] buffer_index: u32,
        #[comptime] input_ident: InputIdent,
        #[comptime] config: G,
    ) -> Job {
        let tiling = config.tiling_dimensions(input_ident);
        let line_size = config.global_line_size(input_ident);
        let num_planes = config.num_planes();
        let num_tiles = tiling.tile_count();
        let plane_dim = config.plane_dim();

        let num_tiles_per_plane = comptime!(num_tiles / num_planes);
        let num_lines_per_tile = comptime!(tiling.tile_size() / line_size);
        let num_lines_per_plane = num_lines_per_tile * num_tiles_per_plane;
        let num_lines_per_unit = num_lines_per_plane / plane_dim;

        let num_stages = config.num_stages();
        let stage_width = comptime!(match input_ident {
            InputIdent::Lhs => tiling.tile_count_col(),
            InputIdent::Rhs => tiling.tile_count_row(),
        });
        let row_col_stride = num_stages * stage_width;
        let buffer_offset = stage_width * buffer_index;

        comptime! {
            println!("-------------");
            println!("input_ident: {:?}", input_ident);
            println!("line_size: {:?}", line_size);
            println!("num_planes: {:?}", num_planes);
            println!("num_tiles: {:?}", num_tiles);
            println!("plane_dim: {:?}", plane_dim);
            println!("num_tiles_per_plane: {:?}", num_tiles_per_plane);
            println!("num_lines_per_tile: {:?}", num_lines_per_tile);
            println!("num_lines_per_plane: {:?}", num_lines_per_plane);
            println!("num_lines_per_unit: {:?}", num_lines_per_unit);
            println!("num_stages: {:?}", num_stages);
            println!("stage_width: {:?}", stage_width);
            println!("row_col_stride: {:?}", row_col_stride);
            println!("buffer_index: {:?}", buffer_index);
            println!("buffer_offset: {:?}", buffer_offset);
        }

        // 0..4 * 8 = 0,8,..32
        let starting_tile_within_stage = UNIT_POS_Y * num_tiles_per_plane;
        // 0,8,..32 / 4 = 0,2,4,6
        let row_col_index = starting_tile_within_stage / stage_width;
        // 0,8,..32 % 4 = 0
        let inner_offset = starting_tile_within_stage % stage_width;
        // 0,2,4,6 * 8 + 0 + 0 = 0,16,32,48 OR 4,20,36,52
        let num_tiles_to_skip = row_col_index * row_col_stride + inner_offset + buffer_offset;
        // 0,16,32,48 * 8 = 0,128,256,384 OR 32,160,288,416
        let num_lines_to_skip = num_tiles_to_skip * num_lines_per_tile;

        Job {
            num_tiles_to_skip,
            num_lines_to_skip,
            buffer_index,
            row_col_stride,
            stage_width,
            num_lines_per_tile,
            num_lines_per_unit,
            plane_dim: config.plane_dim(),
            line_size,
            input_ident,
        }
    }
}

#[derive(CubeType, Clone, Copy)]
pub struct Job {
    num_tiles_to_skip: u32,
    num_lines_to_skip: u32,

    #[cube(comptime)]
    buffer_index: u32,
    #[cube(comptime)]
    row_col_stride: u32,
    #[cube(comptime)]
    stage_width: u32,
    #[cube(comptime)]
    num_lines_per_tile: u32,
    #[cube(comptime)]
    num_lines_per_unit: u32,
    #[cube(comptime)]
    plane_dim: u32,
    #[cube(comptime)]
    line_size: u32,
    #[cube(comptime)]
    input_ident: InputIdent,
}

#[cube]
impl<MP: MatmulPrecision, TO: TilingOrder> LoadingJob<MP, ContiguousTilingLayout<TO>> for Job {
    fn execute_task<G: GlobalConfig>(
        this: &mut Self,
        task_id: u32,
        tensor_reader: &TensorReader<MP::EI>,
        stage: &mut StageMemory<MP::ES, ContiguousTilingLayout<TO>>,
        quantization: &CubeOption<Quantization<MP>>,
        #[comptime] config: G,
    ) {
        // 0..2 * 32 + 0..32 = 0..64
        let pos_across_tiles = task_id * this.plane_dim + UNIT_POS_X;
        // 0..64 / 8 = 0...1...2...,7...
        let nth_tile_for_this_plane = pos_across_tiles / this.num_lines_per_tile;
        // 0..64 % 8 = 0..7,0..7,0..7,0..7,0..7,0..7,0..7,0..7
        let line_index_within_tile = pos_across_tiles % this.num_lines_per_tile;

        // 0...1...2...,7... / 4 = 0....................1...................
        // 0000000011111111222222223333333344444444555555556666666677777777 / 4 =
        // 0000000000000000000000000000000011111111111111111111111111111111
        let row_col_index_local = nth_tile_for_this_plane / this.stage_width;
        // 0...1...2...,7... % 4 = 0...1...2...3...0...1...2...3...
        // 0000000011111111222222223333333344444444555555556666666677777777 % 4 =
        // 0000000011111111222222223333333300000000111111112222222233333333
        let inner_offset = nth_tile_for_this_plane % this.stage_width;
        // 0....................1................... * 8 + 0...1...2...3...0...1...2...3...
        // 0000000000000000000000000000000088888888888888888888888888888888 +
        // 0000000011111111222222223333333300000000111111112222222233333333 =
        // 000000001111111122222222333333338888888899999999AAAAAAAABBBBBBBB
        let num_tiles_to_skip_local = row_col_index_local * this.row_col_stride + inner_offset;
        // 0,16,32,48 + 000000001111111122222222333333338888888899999999AAAAAAAABBBBBBBB
        // OR
        // 4,20,36,52 + 000000001111111122222222333333338888888899999999AAAAAAAABBBBBBBB
        // Are they all there?
        // 0,1,2,3
        // 4,5,6,7
        // 8,9,10,11
        // 12,13,14,15 etc. yes
        let nth_tile_global = this.num_tiles_to_skip + num_tiles_to_skip_local;

        let (total_tile_count_row, total_tile_count_col) = match comptime!(this.input_ident) {
            InputIdent::Lhs => (
                comptime!(config.tiling_dimensions(this.input_ident).tile_count_row()),
                comptime!(
                    config.tiling_dimensions(this.input_ident).tile_count_col()
                        * config.num_stages()
                ),
            ),
            InputIdent::Rhs => (
                comptime!(
                    config.tiling_dimensions(this.input_ident).tile_count_row()
                        * config.num_stages()
                ),
                comptime!(config.tiling_dimensions(this.input_ident).tile_count_col()),
            ),
        };

        let tile = TO::to_row_col::<G::SmmConfig>(
            nth_tile_global,
            total_tile_count_row,
            total_tile_count_col,
            comptime!(this.input_ident.as_ident()),
            config.to_smm_config(),
        );

        let num_lines_to_skip_global = nth_tile_global * this.num_lines_per_tile;

        Job::load_and_store_line::<MP, TO, G>(
            this,
            tile,
            // 0..7,0..7,0..7,0..7,0..7,0..7,0..7,0..7
            line_index_within_tile,
            num_lines_to_skip_global,
            tensor_reader,
            stage,
            quantization,
            config,
        );
    }

    fn task_count(this: &Self) -> comptime_type!(u32) {
        comptime!(this.num_lines_per_unit)
    }
}

#[cube]
impl Job {
    #[allow(clippy::too_many_arguments)]
    fn load_and_store_line<MP: MatmulPrecision, TO: TilingOrder, G: GlobalConfig>(
        this: &Self,
        tile: (u32, u32),
        line_index_within_tile: u32,
        num_lines_to_skip_global: u32,
        tensor_reader: &TensorReader<MP::EI>,
        stage: &mut StageMemory<MP::ES, ContiguousTilingLayout<TO>>,
        quantization: &CubeOption<Quantization<MP>>,
        #[comptime] config: G,
    ) {
        let line_read = tensor_reader.load_coalesced_in_tile::<G>(
            tile.0,
            tile.1,
            line_index_within_tile * this.line_size,
            this.input_ident,
            config,
        );

        let offset = line_index_within_tile + num_lines_to_skip_global;

        stage.as_slice_mut(this.line_size)[offset] = match quantization {
            CubeOption::Some(quantization) => quantization.dequantize(line_read, this.input_ident),
            CubeOption::None => Line::cast_from(line_read),
        };
    }
}
