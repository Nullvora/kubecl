use cubecl_core as cubecl;
use cubecl_core::prelude::*;

use crate::instructions::ReduceRequirements;

use super::{
    ArgAccumulator, ReduceCoordinate, ReduceCoordinateExpand, ReduceFamily, ReduceInstruction,
    lowest_coordinate_matching,
};

/// Compute the coordinate of the maximum item returning the smallest coordinate in case of equality.
#[derive(Debug, CubeType, Clone)]
pub struct ArgMax {}

#[cube]
impl ArgMax {
    /// Compare two pairs of items and coordinates and return a new pair
    /// where each element in the lines is the maximal item with its coordinate.
    /// In case of equality, the lowest coordinate is selected.
    pub fn choose_argmax<N: Numeric>(
        items0: Line<N>,
        coordinates0: Line<u32>,
        items1: Line<N>,
        coordinates1: Line<u32>,
    ) -> (Line<N>, Line<u32>) {
        let to_keep = select_many(
            items0.equal(items1),
            coordinates0.less_than(coordinates1),
            items0.greater_than(items1),
        );
        let items = select_many(to_keep, items0, items1);
        let coordinates = select_many(to_keep, coordinates0, coordinates1);
        (items, coordinates)
    }
}

impl ReduceFamily for ArgMax {
    type Instruction<In: Numeric> = Self;
    type Config = ();
}

#[cube]
impl<In: Numeric> ReduceInstruction<In> for ArgMax {
    type AccumulatorItem = (Line<In>, Line<u32>);
    type SharedAccumulator = ArgAccumulator<In>;
    type Config = ();

    fn requirements(_this: &Self) -> ReduceRequirements {
        ReduceRequirements { coordinates: true }
    }

    fn from_config(_config: Self::Config) -> Self {
        ArgMax {}
    }

    fn null_input(_this: &Self, #[comptime] line_size: u32) -> Line<In> {
        Line::empty(line_size).fill(In::min_value())
    }

    fn null_accumulator(this: &Self, #[comptime] line_size: u32) -> Self::AccumulatorItem {
        (
            Self::null_input(this, line_size),
            Line::empty(line_size).fill(u32::MAX),
        )
    }

    fn assign_accumulator(
        _this: &Self,
        destination: &mut Self::AccumulatorItem,
        source: &Self::AccumulatorItem,
    ) {
        destination.0 = source.0;
        destination.1 = source.1;
    }

    fn reduce(
        _this: &Self,
        accumulator: &Self::AccumulatorItem,
        item: Line<In>,
        coordinate: ReduceCoordinate,
        #[comptime] use_planes: bool,
    ) -> Self::AccumulatorItem {
        let coordinate = match coordinate {
            ReduceCoordinate::Required(val) => val,
            ReduceCoordinate::NotRequired => {
                comptime! {panic!("Coordinates are required for ArgMin")};
                #[allow(unreachable_code)]
                Line::new(0)
            }
        };

        let (candidate_item, candidate_coordinate) = if use_planes {
            let candidate_item = plane_max(item);
            let candidate_coordinate = lowest_coordinate_matching(candidate_item, item, coordinate);
            (candidate_item, candidate_coordinate)
        } else {
            (item, coordinate)
        };

        Self::choose_argmax(
            candidate_item,
            candidate_coordinate,
            accumulator.0,
            accumulator.1,
        )
    }

    fn fuse_accumulators(
        _this: &Self,
        lhs: Self::AccumulatorItem,
        rhs: Self::AccumulatorItem,
    ) -> Self::AccumulatorItem {
        Self::choose_argmax(lhs.0, lhs.1, rhs.0, rhs.1)
    }

    fn merge_line<Out: Numeric>(
        _this: &Self,
        accumulator: Self::AccumulatorItem,
        _shape_axis_reduce: u32,
    ) -> Out {
        let line_size = accumulator.0.size();
        if comptime!(line_size > 1) {
            let mut max = In::min_value();
            let mut coordinate = u32::MAX.runtime();
            #[unroll]
            for k in 0..line_size {
                let acc_element = accumulator.0[k];
                let acc_coordinate = accumulator.1[k];
                if acc_element == max && acc_coordinate < coordinate {
                    coordinate = acc_coordinate;
                } else if acc_element > max {
                    max = acc_element;
                    coordinate = acc_coordinate;
                }
            }
            Out::cast_from(coordinate)
        } else {
            Out::cast_from(accumulator.1)
        }
    }

    fn to_output_perpendicular<Out: Numeric>(
        _this: &Self,
        accumulator: Self::AccumulatorItem,
        _shape_axis_reduce: u32,
    ) -> Line<Out> {
        Line::cast_from(accumulator.1)
    }
}
