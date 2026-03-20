use clap::Parser;

mod commands;
mod output;

#[derive(Parser)]
#[command(
    name = "wt",
    about = "W0rkTree — Next-generation version control",
    version,
    long_about = "W0rkTree is a complete Git replacement with native multi-tenant architecture,\nstructured project management, and real-time collaboration."
)]
struct Cli {
    #[command(subcommand)]
    command: commands::Commands,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    if let Err(e) = commands::execute(cli.command).await {
        output::format::print_error(&e.to_string());
        std::process::exit(1);
    }
}
