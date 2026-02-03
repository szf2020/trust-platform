//! CLI definitions for trust-runtime.

use clap::{ArgAction, Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "trust-runtime",
    version,
    about = "Structured Text runtime CLI",
    infer_subcommands = true,
    arg_required_else_help = false,
    after_help = "Examples:\n  trust-runtime                       # start (first run opens setup)\n  trust-runtime --verbose             # show startup details\n  trust-runtime ui --project ./my-plc # terminal UI\n  trust-runtime play --project ./my-plc # compatibility"
)]
pub struct Cli {
    /// Show verbose startup details.
    #[arg(long, short, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a runtime instance (PLC mode).
    Run {
        /// Project folder directory.
        #[arg(long = "project", alias = "bundle")]
        project: Option<PathBuf>,
        /// Configuration entry file (dev mode).
        #[arg(long)]
        config: Option<PathBuf>,
        /// Root directory for ST sources (dev mode).
        #[arg(long)]
        runtime_root: Option<PathBuf>,
        /// Restart mode on startup.
        #[arg(long, default_value = "cold")]
        restart: String,
    },
    /// Start the runtime with project auto-detection (production UX).
    #[command(
        after_help = "Examples:\n  trust-runtime play\n  trust-runtime play --project ./my-plc\n  trust-runtime play --restart warm"
    )]
    Play {
        /// Project folder directory (auto-creates a default project if missing).
        #[arg(long = "project", alias = "bundle")]
        project: Option<PathBuf>,
        /// Restart mode on startup.
        #[arg(long, default_value = "cold")]
        restart: String,
        /// Force-enable the interactive console (TTY only).
        #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_console")]
        console: bool,
        /// Disable the interactive console.
        #[arg(long, action = ArgAction::SetTrue, conflicts_with = "console")]
        no_console: bool,
        /// Use beginner mode (limited controls).
        #[arg(long, action = ArgAction::SetTrue)]
        beginner: bool,
    },
    /// Interactive TUI for monitoring and control.
    Ui {
        /// Project folder directory (auto-detect if omitted).
        #[arg(long = "project", alias = "bundle")]
        project: Option<PathBuf>,
        /// Control endpoint override (tcp://host:port or unix://path).
        #[arg(long)]
        endpoint: Option<String>,
        /// Control auth token (overrides project value).
        #[arg(long)]
        token: Option<String>,
        /// UI refresh interval in milliseconds.
        #[arg(long, default_value = "250")]
        refresh: u64,
        /// Read-only mode (monitor only).
        #[arg(long)]
        no_input: bool,
        /// Beginner mode (Play/Stop/Download/Debug only).
        #[arg(long)]
        beginner: bool,
    },
    /// Send control commands to a running runtime.
    Ctl {
        /// Project folder directory (to read control endpoint).
        #[arg(long = "project", alias = "bundle")]
        project: Option<PathBuf>,
        /// Control endpoint (tcp://host:port or unix://path).
        #[arg(long)]
        endpoint: Option<String>,
        /// Control auth token (overrides project value).
        #[arg(long)]
        token: Option<String>,
        #[command(subcommand)]
        action: ControlAction,
    },
    /// Validate a project folder (config + bytecode).
    Validate {
        /// Project folder directory.
        #[arg(long = "project", alias = "bundle")]
        project: PathBuf,
    },
    /// Build program.stbc from project sources.
    Build {
        /// Project folder directory (defaults to auto-detect or current directory).
        #[arg(long = "project", alias = "bundle")]
        project: Option<PathBuf>,
        /// Sources directory override (defaults to <project>/sources).
        #[arg(long)]
        sources: Option<PathBuf>,
    },
    /// Initialize system IO configuration (writes /etc/trust/io.toml).
    #[command(
        after_help = "Examples:\n  trust-runtime setup\n  trust-runtime setup --driver gpio --force\n  trust-runtime setup --path ./io.toml"
    )]
    Setup {
        /// Override driver selection (default is auto-detect).
        #[arg(long)]
        driver: Option<String>,
        /// Override GPIO backend (e.g., sysfs).
        #[arg(long)]
        backend: Option<String>,
        /// Override output path (default: system io.toml).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Overwrite existing system config.
        #[arg(long)]
        force: bool,
    },
    /// Guided wizard to create a new project folder.
    #[command(alias = "init")]
    Wizard {
        /// Target directory (defaults to current directory).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Start the runtime after creating the project folder.
        #[arg(long)]
        start: bool,
    },
    /// Commit project changes with a human-friendly summary.
    Commit {
        /// Project folder directory (defaults to current directory).
        #[arg(long = "project", alias = "bundle")]
        project: Option<PathBuf>,
        /// Commit message (skip prompt).
        #[arg(long)]
        message: Option<String>,
        /// Print summary without committing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Deploy a project folder into a versioned store with rollback support.
    Deploy {
        /// Source project folder directory.
        #[arg(long = "project", alias = "bundle")]
        project: PathBuf,
        /// Deployment root (defaults to current directory).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Custom deployment label (defaults to project-<timestamp>).
        #[arg(long)]
        label: Option<String>,
        /// Restart mode after deployment (optional).
        #[arg(long)]
        restart: Option<String>,
    },
    /// Roll back to the previous project version in a deployment root.
    Rollback {
        /// Deployment root (defaults to current directory).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Subcommand)]
#[command(infer_subcommands = true)]
pub enum ControlAction {
    Status,
    Health,
    Stats,
    Pause,
    Resume,
    StepIn,
    StepOver,
    StepOut,
    BreakpointsSet { source: String, lines: Vec<u32> },
    BreakpointsClear { source: String },
    BreakpointsList,
    IoRead,
    IoWrite { address: String, value: String },
    IoForce { address: String, value: String },
    IoUnforce { address: String },
    Eval { expr: String },
    Set { target: String, value: String },
    Restart { mode: String },
    Shutdown,
    ConfigGet,
    ConfigSet { key: String, value: String },
}
