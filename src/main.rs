use clap::Parser;
use syncfleet::cli;

#[tokio::main]
async fn main() {
    let args = cli::Cli::parse();

    if let Err(e) = cli::run(args).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
