//! sshpdf: a PDF viewer for any terminal, drawn with characters only.

mod ansi;
mod cli;
mod config;
mod dump;
mod logger;
mod pngout;
mod probe;
mod session;
mod state;
mod tui;

use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Parser;

use crate::cli::Cli;
use crate::config::{Config, Settings};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let log_path = cli
        .log
        .clone()
        .or_else(|| std::env::var_os("SSHPDF_LOG").map(Into::into));
    if let Some(path) = log_path
        && let Err(e) = logger::init(&path)
    {
        eprintln!("sshpdf: cannot write log {}: {e}", path.display());
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sshpdf: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let config_path = cli.config.clone().or_else(config::default_path);
    let config = match &config_path {
        Some(p) => Config::load(p)?,
        None => Config::default(),
    };
    let settings = Settings::resolve(&cli, &config, sshpdf_term::caps::detect())?;
    if cli.probe {
        return probe::run(&cli, &settings, config);
    }
    if let Some(input) = &cli.render_ansi {
        return dump::render_ansi(&cli, &settings, input);
    }
    if cli.dump {
        return dump::dump(&cli, &settings);
    }
    if cli.dump_png {
        return dump::dump_png(&cli, &settings);
    }
    if cli.text {
        return dump::text(&cli);
    }
    if cli.file.is_none() {
        bail!("no FILE given (see --help)");
    }
    tui::run(&cli, settings)
}
