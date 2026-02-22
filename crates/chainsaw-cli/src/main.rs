use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set the mode
    Set {
        /// Mode
        /// "integrated"
        /// "hybrid"
        mode: String,
    },
    /// Get the current mode
    Get,
    /// List supported modes
    List,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let connection = zbus::connection::Builder::system()?.build().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        "com.chainsaw.daemon",
        "/com/chainsaw/daemon",
        "com.chainsaw.daemon",
    )
    .await?;

    match args.command {
        Commands::Set { mode } => {
            let response: String = proxy.call("SetMode", &(mode,)).await?;
            println!("{}", response);
        }
        Commands::Get => {
            let current_mode: String = proxy.call("GetMode", &()).await?;
            println!("Current gpu mode: {}", current_mode);
        }
        Commands::List => {
            let response: Vec<String> = proxy.call("ListMode", &()).await?;
            for mode in response {
                println!("{}", mode);
            }
        }
    }

    Ok(())
}
