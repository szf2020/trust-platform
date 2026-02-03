//! CLI entrypoint for ST runtime.

#[path = "trust-runtime/build.rs"]
mod build;
#[path = "trust-runtime/cli.rs"]
mod cli;
#[path = "trust-runtime/commit.rs"]
mod commit;
#[path = "trust-runtime/completions.rs"]
mod completions;
#[path = "trust-runtime/ctl.rs"]
mod ctl;
#[path = "trust-runtime/deploy.rs"]
mod deploy;
#[path = "trust-runtime/git.rs"]
mod git;
#[path = "trust-runtime/prompt.rs"]
mod prompt;
#[path = "trust-runtime/run.rs"]
mod run;
#[path = "trust-runtime/setup.rs"]
mod setup;
#[path = "trust-runtime/style.rs"]
mod style;
#[path = "trust-runtime/wizard.rs"]
mod wizard;

use clap::error::ErrorKind;
use clap::Parser;

use cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    if let Err(err) = run() {
        let message = format_error_with_tip(&err);
        eprintln!("{}", style::error(format!("Error: {message}")));
        std::process::exit(1);
    }
    Ok(())
}

fn run() -> anyhow::Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    let used_bundle_flag = raw_args
        .iter()
        .any(|arg| arg == "--bundle" || arg.starts_with("--bundle="));
    let cli = match Cli::try_parse_from(&raw_args) {
        Ok(cli) => cli,
        Err(err) => {
            if err.kind() == ErrorKind::InvalidSubcommand {
                if let Some(input) = raw_args.get(1) {
                    if let Some(suggestion) = suggest_subcommand(input) {
                        eprintln!("Did you mean: {suggestion}?");
                    }
                }
            }
            err.exit();
        }
    };
    if used_bundle_flag {
        eprintln!(
            "{}",
            style::warning("Warning: --bundle is deprecated. Use --project instead.")
        );
    }
    match cli.command {
        None => run::run_default(cli.verbose),
        Some(Command::Run {
            project,
            config,
            runtime_root,
            restart,
        }) => run::run_runtime(
            project,
            config,
            runtime_root,
            restart,
            cli.verbose,
            false,
            run::ConsoleMode::Disabled,
            false,
        ),
        Some(Command::Play {
            project,
            restart,
            console,
            no_console,
            beginner,
        }) => {
            let console_mode = if no_console {
                run::ConsoleMode::Disabled
            } else if console {
                run::ConsoleMode::Enabled
            } else {
                run::ConsoleMode::Auto
            };
            run::run_play(project, restart, cli.verbose, console_mode, beginner)
        }
        Some(Command::Ui {
            project,
            endpoint,
            token,
            refresh,
            no_input,
            beginner,
        }) => trust_runtime::ui::run_ui(project, endpoint, token, refresh, no_input, beginner),
        Some(Command::Ctl {
            project,
            endpoint,
            token,
            action,
        }) => ctl::run_control(project, endpoint, token, action),
        Some(Command::Validate { project }) => run::run_validate(project),
        Some(Command::Build { project, sources }) => build::run_build(project, sources),
        Some(Command::Setup {
            driver,
            backend,
            path,
            force,
        }) => setup::run_setup(driver, backend, path, force),
        Some(Command::Wizard { path, start }) => wizard::run_wizard(path, start),
        Some(Command::Commit {
            project,
            message,
            dry_run,
        }) => commit::run_commit(project, message, dry_run),
        Some(Command::Deploy {
            project,
            root,
            label,
            restart,
        }) => {
            let result = deploy::run_deploy(project, root, label)?;
            if let Some(mode) = restart {
                ctl::run_control(
                    Some(result.current_bundle),
                    None,
                    None,
                    cli::ControlAction::Restart { mode },
                )?;
            }
            Ok(())
        }
        Some(Command::Rollback { root }) => deploy::run_rollback(root),
        Some(Command::Completions { shell }) => completions::run_completions(shell),
    }
}

fn suggest_subcommand(input: &str) -> Option<&'static str> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let candidates = [
        "run",
        "play",
        "setup",
        "wizard",
        "ui",
        "ctl",
        "validate",
        "build",
        "deploy",
        "rollback",
        "commit",
        "completions",
    ];
    let mut best = None;
    let mut best_score = usize::MAX;
    for candidate in candidates {
        let score = levenshtein(input, candidate);
        if score < best_score {
            best_score = score;
            best = Some(candidate);
        }
    }
    if best_score <= 2 {
        best
    } else {
        None
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.clone_from_slice(&curr);
    }
    prev[b.len()]
}

fn format_error_with_tip(err: &anyhow::Error) -> String {
    let message = err.to_string();
    let tip = if message.contains("/etc/trust") && message.contains("Permission denied") {
        Some("Tip: run `sudo trust-runtime setup --force` to write system I/O, or run `trust-runtime --project <dir>` for a local project.")
    } else if message.contains("invalid project folder") {
        Some("Tip: run `trust-runtime` in an empty folder or `trust-runtime wizard --path <project-folder>` to create one.")
    } else if message.contains("invalid config") {
        Some(
            "Tip: check runtime.toml/io.toml or run `trust-runtime wizard` to regenerate defaults.",
        )
    } else if message.contains("auth_token") && message.contains("tcp control endpoint") {
        Some("Tip: set runtime.control.auth_token in runtime.toml for TCP control.")
    } else if message.contains("debug disabled") {
        Some("Tip: set runtime.control.debug_enabled=true in runtime.toml to use pause/step controls.")
    } else {
        None
    };
    match tip {
        Some(tip) => format!("{message}\n{tip}"),
        None => message,
    }
}
