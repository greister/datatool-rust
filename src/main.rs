#![allow(dead_code)]

mod cli;
mod config;
mod day;
mod formats;
mod min;
mod tick;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    env_logger::init();
    let args = cli::Args::parse();
    args.run()
}
