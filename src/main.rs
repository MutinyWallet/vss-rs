use crate::config::Config;
use clap::Parser;

mod config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _config: Config = Config::parse();

    Ok(())
}
