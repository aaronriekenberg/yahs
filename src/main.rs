use std::path::PathBuf;

use clap::Parser;

use yahs::{config, logging, server};

/// yahs — yet another http server
#[derive(Debug, Parser)]
#[command(
    name = "yahs",
    version,
    about = "yet another http server: a configurable, extensible web server"
)]
struct Cli {
    /// Path to the TOML configuration file.
    #[arg(short, long, default_value = "yahs.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load configuration first so we can set up logging with the configured level.
    let config = config::load_config(&cli.config)?;

    // Initialize tracing.
    logging::init_logging(&config.logging.level);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        config = %cli.config.display(),
        "yahs starting"
    );

    server::run_server(config).await?;

    Ok(())
}
