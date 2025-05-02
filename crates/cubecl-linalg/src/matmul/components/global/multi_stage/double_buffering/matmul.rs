use crate::matmul::components::global;
use crate::matmul::components::global::Quantization;
use crate::matmul::components::global::load::{
    BufferId, SyncBufferLoader, SyncBufferLoaderJob, SyncBufferLoadingStrategy,
};
use crate::matmul::components::global::multi_stage::double_buffering::DoubleBufferingGlobalConfig;
use crate::matmul::components::global::output_loader::Unloader;
use crate::matmul::components::global::{GlobalConfig, ZeroAccumulatorLoader};
use crate::matmul::components::stage::BufferReader;
use crate::matmul::components::stage::StageEvent;
use crate::matmul::components::stage::StageEventListener;
use crate::matmul::components::{
    Ident, InputIdent, InvalidConfigError, MatmulConfigFactory, MatmulPrecision, MatmulProblem,
    stage,
};
use crate::matmul::components::{global::GlobalMatmulFamily, stage::BufferReaderFamily};
use crate::matmul::kernels::MatmulAvailabilityError;
use crate::matmul::kernels::matmul::LoadingPrecomputeStrategy;
use cubecl_core as cubecl;
use cubecl_core::prelude::*;
use cubecl_std::tensor::r#virtual::{ReadWrite, VirtualTensor};
use cubecl_std::{CubeOption, div_ceil};
use std::marker::PhantomData;

pub struct DoubleBufferingMatmulFamily<
    SMM: stage::StageMatmulFamily,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
> {
    _stage_matmul: PhantomData<SMM>,
    _lhs_loading: PhantomData<LL>,
    _rhs_loading: PhantomData<RL>,
}

impl<SMM, LL, RL> GlobalMatmulFamily for DoubleBufferingMatmulFamily<SMM, LL, RL>
where
    SMM: stage::StageMatmulFamily<LhsReader = BufferReaderFamily, RhsReader = BufferReaderFamily>,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
{
    type Matmul<MP: MatmulPrecision> =
        DoubleBufferingMatmul<MP, SMM::Matmul<MP, LL::TilingLayout, RL::TilingLayout>, LL, RL>;
}

impl<SMM, LL, RL> MatmulConfigFactory for DoubleBufferingMatmulFamily<SMM, LL, RL>
where
    SMM: stage::StageMatmulFamily,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
{
    type Input = (SMM::Input, LoadingPrecomputeStrategy);
    type Config = DoubleBufferingGlobalConfig<SMM::Config>;

    fn check_config(config: &Self::Config) -> Result<(), InvalidConfigError> {
        LL::check::<Self::Config>(config, Ident::Lhs)?;
        RL::check::<Self::Config>(config, Ident::Rhs)?;

        SMM::check_config(&config.to_smm_config())
    }

    fn check_availability<R: Runtime, MP: MatmulPrecision>(
        client: &ComputeClient<R::Server, R::Channel>,
        config: &Self::Config,
    ) -> Result<(), MatmulAvailabilityError> {
        SMM::check_availability::<R, MP>(client, &config.smm_config)
    }

    fn make_config(
        input: Self::Input,
        problem: &MatmulProblem,
        cube_dim: &CubeDim,
        cube_count: &CubeCount,
        quantized: bool,
    ) -> Self::Config {
        let smm_config = SMM::make_config(input.0, problem, cube_dim, cube_count, quantized);
        let stage_shape = SMM::stage_shape(&smm_config);

        DoubleBufferingGlobalConfig::new(
            smm_config,
            problem.m as u32 % stage_shape.m != 0,
            problem.n as u32 % stage_shape.n != 0,
            problem.k as u32 % (2 * stage_shape.k) != 0,
            problem.lhs_layout,
            problem.rhs_layout,
            problem.lhs_line_size as u32,
            problem.rhs_line_size as u32,
            problem.out_line_size as u32,
            cube_dim.y,
            input.1,
        )
    }
}

/// Performs matrix multiplication at the global level, with planes pipelining their work using two buffers:
/// While they trigger a load event from global memory to shared memory on buffer A,
/// they trigger a computation event from tensor cores on buffer B. Then buffers are switched.
pub struct DoubleBufferingMatmul<
    MP: MatmulPrecision,
    SMM: stage::StageMatmul<MP>,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
> {
    _ms: PhantomData<MP>,
    _stage_matmul: PhantomData<SMM>,
    _lhs_loading: PhantomData<LL>,
    _rhs_loading: PhantomData<RL>,
}

#[cube]
impl<MP: MatmulPrecision, SMM, LL, RL> global::GlobalMatmul<MP>
    for DoubleBufferingMatmul<MP, SMM, LL, RL>
where
    SMM: stage::StageMatmul<
            MP,
            LhsReader = BufferReader<MP::ES, LL::TilingLayout>,
            RhsReader = BufferReader<MP::ES, RL::TilingLayout>,
        >,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
{
    type Config = DoubleBufferingGlobalConfig<SMM::Config>;
    type LhsLoader = SyncBufferLoader<MP, Self::Config, LL>;
    type RhsLoader = SyncBufferLoader<MP, Self::Config, RL>;
    type AccumulatorLoader = ZeroAccumulatorLoader;
    type Out = Unloader<MP::EO>;
    type Accumulator = SMM::Accumulator;

    fn execute(
        mut lhs_loader: Self::LhsLoader,
        mut rhs_loader: Self::RhsLoader,
        mut out_unloader: Self::Out,
        acc: &mut Self::Accumulator,
        k_range: (u32, u32),
        #[comptime] config: Self::Config,
    ) {
        let buffer_step = config.tiling_dimensions(Ident::Lhs).total_col();
        let loop_step = buffer_step * 2;
        let range = k_range.1 - k_range.0;
        let needed_stage_matmuls = div_ceil(range, buffer_step);

        // Algorithm assumes an even number of stages
        let num_stage_matmuls = needed_stage_matmuls + (needed_stage_matmuls % 2);
        let num_loops = (num_stage_matmuls - 2) / 2;

        SMM::zero_accumulator(acc, config.to_smm_config());
        let (mut lhs_tile_a, mut rhs_tile_a) = SMM::init_tile_inputs(config.to_smm_config());
        let (mut lhs_tile_b, mut rhs_tile_b) = SMM::init_tile_inputs(config.to_smm_config());

        let lhs_reader_a = Self::LhsLoader::reader(&lhs_loader, BufferId::A);
        let lhs_reader_b = Self::LhsLoader::reader(&lhs_loader, BufferId::B);
        let rhs_reader_a = Self::RhsLoader::reader(&rhs_loader, BufferId::A);
        let rhs_reader_b = Self::RhsLoader::reader(&rhs_loader, BufferId::B);

        Self::LhsLoader::fill_stage(&mut lhs_loader, BufferId::A, config);
        Self::RhsLoader::fill_stage(&mut rhs_loader, BufferId::A, config);

        sync_units();

        for _ in 0..num_loops {
            SMM::execute_with_listener::<
                DoubleBufferingEventListener<
                    SyncBufferLoader<MP, Self::Config, LL>,
                    SyncBufferLoader<MP, Self::Config, RL>,
                    Self::Config,
                >,
            >(
                &lhs_reader_a,
                &rhs_reader_a,
                &mut lhs_tile_a,
                &mut rhs_tile_a,
                acc,
                config.to_smm_config(),
                DoubleBufferingEventListener::new(BufferId::B, &lhs_loader, &rhs_loader, config),
            );

            SyncBufferLoader::<MP, Self::Config, LL>::advance_view(&mut lhs_loader, loop_step);
            SyncBufferLoader::<MP, Self::Config, RL>::advance_view(&mut rhs_loader, loop_step);

            sync_units();

            SMM::execute_with_listener::<
                DoubleBufferingEventListener<
                    SyncBufferLoader<MP, Self::Config, LL>,
                    SyncBufferLoader<MP, Self::Config, RL>,
                    Self::Config,
                >,
            >(
                &lhs_reader_b,
                &rhs_reader_b,
                &mut lhs_tile_b,
                &mut rhs_tile_b,
                acc,
                config.to_smm_config(),
                DoubleBufferingEventListener::new(BufferId::A, &lhs_loader, &rhs_loader, config),
            );

            sync_units();
        }

        SMM::execute_with_listener::<
            DoubleBufferingEventListener<
                SyncBufferLoader<MP, Self::Config, LL>,
                SyncBufferLoader<MP, Self::Config, RL>,
                Self::Config,
            >,
        >(
            &lhs_reader_a,
            &rhs_reader_a,
            &mut lhs_tile_a,
            &mut rhs_tile_a,
            acc,
            config.to_smm_config(),
            DoubleBufferingEventListener::new(BufferId::B, &lhs_loader, &rhs_loader, config),
        );

        sync_units();

        SMM::execute(
            &lhs_reader_b,
            &rhs_reader_b,
            &mut lhs_tile_b,
            &mut rhs_tile_b,
            acc,
            config.to_smm_config(),
        );

        SMM::read_accumulator::<Self::Out, Self::Config>(
            acc,
            &mut out_unloader,
            config.to_smm_config(),
            config,
        );
    }

    fn init_lhs_loader(
        lhs: VirtualTensor<MP::EI>,
        x_offset: u32,
        y_offset: u32,
        _nth_batch: u32,
        batch_offset: u32,
        quantization: CubeOption<Quantization<MP>>,
        #[comptime] config: Self::Config,
    ) -> Self::LhsLoader {
        SyncBufferLoader::<MP, Self::Config, LL>::new(
            lhs,
            x_offset,
            y_offset,
            batch_offset,
            quantization,
            InputIdent::Lhs,
            config,
        )
    }

    fn init_rhs_loader(
        rhs: VirtualTensor<MP::EI>,
        x_offset: u32,
        y_offset: u32,
        _nth_batch: u32,
        batch_offset: u32,
        quantization: CubeOption<Quantization<MP>>,
        #[comptime] config: Self::Config,
    ) -> Self::RhsLoader {
        SyncBufferLoader::<MP, Self::Config, RL>::new(
            rhs,
            x_offset,
            y_offset,
            batch_offset,
            quantization,
            InputIdent::Rhs,
            config,
        )
    }

    fn init_unloader(
        out: VirtualTensor<MP::EO, ReadWrite>,
        x_offset: u32,
        y_offset: u32,
        _nth_batch: u32,
        batch_offset: u32,
    ) -> Self::Out {
        Self::Out::new(out, x_offset, y_offset, batch_offset)
    }

    fn init_accumulator(#[comptime] config: Self::Config) -> Self::Accumulator {
        SMM::init_accumulator(config.to_smm_config())
    }

    fn zero_accumulator(acc: &mut Self::Accumulator, #[comptime] config: Self::Config) {
        SMM::zero_accumulator(acc, config.to_smm_config());
    }
}

#[cube]
pub trait LoaderEventListener: CubeType + Clone {
    type State: CubeType;
}

#[cube]
impl<MP: MatmulPrecision, G: GlobalConfig, L: SyncBufferLoadingStrategy> LoaderEventListener
    for SyncBufferLoader<MP, G, L>
{
    type State = SyncBufferLoaderJob<MP, L>;
}

#[derive(CubeType)]
struct DoubleBufferingEventListener<
    Lhs: LoaderEventListener,
    Rhs: LoaderEventListener,
    G: GlobalConfig,
> {
    #[cube(comptime)]
    buffer_id: BufferId,
    loader_lhs: Lhs,
    loader_rhs: Rhs,
    #[cube(comptime)]
    config: G,
    state_lhs: Sequence<Lhs::State>,
    state_rhs: Sequence<Rhs::State>,
}

#[cube]
impl<Lhs: LoaderEventListener, Rhs: LoaderEventListener, G: GlobalConfig>
    DoubleBufferingEventListener<Lhs, Rhs, G>
{
    pub fn new(
        #[comptime] buffer_id: BufferId,
        loader_lhs: &Lhs,
        loader_rhs: &Rhs,
        #[comptime] config: G,
    ) -> DoubleBufferingEventListener<Lhs, Rhs, G> {
        DoubleBufferingEventListener::<Lhs, Rhs, G> {
            buffer_id,
            loader_lhs: comptime![loader_lhs.clone()],
            loader_rhs: comptime![loader_rhs.clone()],
            config,
            state_lhs: Sequence::new(),
            state_rhs: Sequence::new(),
        }
    }
}

#[derive(Clone)]
/// How events are handled for double buffering.
///
/// The goal is to overlap computation instructions with memory instructions.
struct Event {
    /// We execute memory instructions for each [STEP] compute tasks are executed.
    /// The event number to execute the next LHS task.
    lhs: u32,
    /// If no more tasks need to be executed for LHS.
    lhs_completed: bool,
    /// The event number to execute the next RHS task.
    rhs: u32,
    /// If no more tasks need to be executed for RHS.
    rhs_completed: bool,
}

impl CubeDebug for Event {}

const STEP: u32 = 1;

#[cube]
impl<
    MP: MatmulPrecision,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
    G: GlobalConfig,
> StageEventListener
    for DoubleBufferingEventListener<SyncBufferLoader<MP, G, LL>, SyncBufferLoader<MP, G, RL>, G>
{
    fn on_event(this: &mut Self, #[comptime] event: StageEvent) {
        if let StageEvent::TmmCompleted { current, total } = event {
            if comptime![current == 0] {
                this.init();
            }

            let event = this.create_event(total);
            this.on_event(event, current);
        }

        // Cleanup remaining tasks if any.
        if let StageEvent::Finish = event {
            let lhs_job = this.state_lhs.index_mut(0);
            let lhs_num_task_executed = lhs_job.current.read().counter;

            #[unroll]
            for _ in lhs_num_task_executed..lhs_job.num_tasks {
                SyncBufferLoader::execute_task(&mut this.loader_lhs, lhs_job, this.config);
            }

            let rhs_job = this.state_rhs.index_mut(0);
            let rhs_num_task_executed = rhs_job.current.read().counter;

            #[unroll]
            for _ in rhs_num_task_executed..rhs_job.num_tasks {
                SyncBufferLoader::execute_task(&mut this.loader_rhs, rhs_job, this.config);
            }
        }
    }
}

#[cube]
impl<
    MP: MatmulPrecision,
    LL: SyncBufferLoadingStrategy,
    RL: SyncBufferLoadingStrategy,
    G: GlobalConfig,
> DoubleBufferingEventListener<SyncBufferLoader<MP, G, LL>, SyncBufferLoader<MP, G, RL>, G>
{
    fn init(&mut self) {
        let job_lhs = SyncBufferLoader::create_job(&self.loader_lhs, self.buffer_id, self.config);
        let job_rhs = SyncBufferLoader::create_job(&self.loader_rhs, self.buffer_id, self.config);

        self.state_lhs.push(job_lhs);
        self.state_rhs.push(job_rhs);
    }

    fn create_event(&self, #[comptime] total: u32) -> comptime_type!(Event) {
        let lhs_job = self.state_lhs.index(0);
        let rhs_job = self.state_rhs.index(0);
        let num_tasks_total = comptime!(lhs_job.num_tasks + rhs_job.num_tasks);

        let lhs_num_task_executed = lhs_job.current.read().counter;
        let rhs_num_task_executed = rhs_job.current.read().counter;

        comptime! {
            let start = total - (STEP * num_tasks_total);
            Event {
                lhs: lhs_num_task_executed  * STEP + start,
                lhs_completed: lhs_num_task_executed >= lhs_job.num_tasks,
                rhs: rhs_num_task_executed  * STEP + (lhs_job.num_tasks * STEP) + start,
                rhs_completed: rhs_num_task_executed >= rhs_job.num_tasks,
            }
        }
    }

    fn on_event(&mut self, #[comptime] event: Event, #[comptime] current: u32) {
        if comptime![!event.lhs_completed && event.lhs == current] {
            let lhs_job = self.state_lhs.index_mut(0);

            SyncBufferLoader::execute_task(&mut self.loader_lhs, lhs_job, self.config);
        }

        if comptime![!event.rhs_completed && event.rhs == current] {
            let rhs_job = self.state_rhs.index_mut(0);

            SyncBufferLoader::execute_task(&mut self.loader_rhs, rhs_job, self.config);
        }
    }
}
