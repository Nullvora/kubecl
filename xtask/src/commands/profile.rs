use glob::glob;
use tracel_xtask::prelude::*;

#[derive(clap::Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileSubCommand,
}

#[derive(clap::Subcommand, strum::Display)]
pub(crate) enum ProfileSubCommand {
    Bench(BenchOptionsArgs),
}

#[derive(clap::Args)]
pub(crate) struct BenchOptionsArgs {
    #[arg(long)]
    pub bench: String,
    #[arg(long, default_value = "/usr/local/cuda/bin/ncu")]
    pub ncu_path: String,
    #[arg(long, default_value = "/usr/local/cuda/bin/ncu-ui")]
    pub ncu_ui_path: String,
}

pub(crate) struct Profile {}

impl ProfileArgs {
    pub(crate) fn run(&self) -> anyhow::Result<()> {
        Profile::run(&self.command)
    }
}

impl Profile {
    pub(crate) fn run(args: &ProfileSubCommand) -> anyhow::Result<()> {
        Profile {}.execute(args)
    }

    fn execute(&self, command: &ProfileSubCommand) -> anyhow::Result<()> {
        ensure_cargo_crate_is_installed("mdbook", None, None, false)?;
        group!("Profile: {}", command);
        match command {
            ProfileSubCommand::Bench(options) => self.bench(options),
        }?;
        endgroup!();
        Ok(())
    }

    fn bench(&self, options: &BenchOptionsArgs) -> anyhow::Result<()> {
        let get_benches = |bench: &str| {
            let pattern = format!("./target/release/deps/{}-*", bench);
            let files: Vec<_> = glob(&pattern)
                .into_iter()
                .flat_map(|r| r.filter_map(|f| f.ok()))
                .collect();

            files
        };

        get_benches(&options.bench)
            .into_iter()
            .for_each(|f| std::fs::remove_file(f).unwrap());

        run_process(
            "cargo",
            &[
                "build",
                "--bench",
                &options.bench,
                "--release",
                "--features",
                "cuda",
            ],
            None,
            None,
            "Can build bench.",
        )?;

        let bins = get_benches(&options.bench);
        let bin = bins.first().unwrap().as_path().to_str().unwrap();
        let file = format!("target/{}", options.bench);

        run_process(
            "sudo",
            &[
                "BENCH_NUM_SAMPLES=1",
                &options.ncu_path,
                "--config-file",
                "off",
                "--nvtx",
                "--call-stack",
                "--export",
                &file,
                "--force-overwrite",
                bin,
            ],
            None,
            None,
            format!("Should profile {}", options.bench).as_str(),
        )?;

        let output = format!("{}.ncu-rep", file);
        run_process(
            &options.ncu_ui_path,
            &[&output],
            None,
            None,
            format!("Should open results for {}", options.bench).as_str(),
        )
    }
}
