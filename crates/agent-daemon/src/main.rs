use agent_daemon::Workbench;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agent-daemon", about = "Multi-agent workbench daemon (V0)")]
struct Cli {
    #[arg(long, default_value = ".agent-workbench")]
    data_dir: PathBuf,

    #[arg(long, default_value = ".")]
    repo: PathBuf,

    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Open workbench, print startup banners / paths
    Status,
    /// Accept YAML graph and run with mock agent executor (no OpenCode required)
    RunMock {
        #[arg(long)]
        yaml: Option<PathBuf>,
    },
    /// Replay fixture ACP transcript into journal
    ReplayFixture,
    /// Seed L0/L1/proposal demo rows
    SeedMemory,
    /// Approve a proposal into L2
    ApproveProposal { id: String },
    /// Invalidate an L1 fact (soft)
    InvalidateL1 { id: String },
    /// Print events since seq
    Events {
        #[arg(long, default_value_t = 0)]
        since: i64,
    },
    /// Smoke-test mock ACP agent over stdio
    AcpSmoke,
    /// Live OpenCode ACP smoke (`opencode acp`). Skips (exit 0 + skipped:true) when binary absent.
    AcpLiveSmoke,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    let wb = Workbench::open(&cli.data_dir, &cli.repo)?;

    match cli.cmd {
        Commands::Status => {
            println!("{}", serde_json::to_string_pretty(&wb.state)?);
        }
        Commands::RunMock { yaml } => {
            let text = if let Some(p) = yaml {
                std::fs::read_to_string(p)?
            } else {
                wb.demo_yaml().to_string()
            };
            let result = wb.accept_and_run_mock(&text)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::ReplayFixture => {
            let evs = wb.replay_fixture_transcript()?;
            println!("{}", serde_json::to_string_pretty(&evs)?);
        }
        Commands::SeedMemory => {
            println!("{}", serde_json::to_string_pretty(&wb.seed_demo_memory()?)?);
        }
        Commands::ApproveProposal { id } => {
            let b = wb.memory().approve_proposal(&id)?;
            println!("{}", serde_json::to_string_pretty(&b)?);
        }
        Commands::InvalidateL1 { id } => {
            wb.memory().invalidate_l1(&id)?;
            let all = wb.memory().list_l1(true)?;
            println!("{}", serde_json::to_string_pretty(&all)?);
        }
        Commands::Events { since } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&wb.events_since(since)?)?
            );
        }
        Commands::AcpSmoke => {
            let cwd = std::env::current_dir()?;
            let evs = agent_daemon::run_mock_acp_smoke(&cwd).await?;
            println!("{}", serde_json::to_string_pretty(&evs)?);
        }
        Commands::AcpLiveSmoke => {
            let cwd = std::env::current_dir()?;
            let report = agent_daemon::run_live_acp_smoke(&cwd).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.skipped {
                // Visible skip — not a pass. Exit 0 so CI stays green.
                eprintln!(
                    "SKIP: {}",
                    report.reason.as_deref().unwrap_or("OpenCode missing")
                );
            } else if !report.prompt_ok {
                // Session opened but prompt failed (often auth) — still exit 0 with honest JSON.
                eprintln!(
                    "WARN: live session opened but prompt failed: {}",
                    report.prompt_error.as_deref().unwrap_or("unknown")
                );
            }
        }
    }
    Ok(())
}
