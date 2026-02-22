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
    /// List GPUs in a table
    List,
    /// GPU operations
    Gpu {
        /// GPU numeric id
        id: u32,
        #[command(subcommand)]
        command: GpuCommands,
    },
    /// List supported modes
    ListModes,
}

#[derive(Subcommand)]
enum GpuCommands {
    /// Block or unblock a GPU
    Block {
        /// on/off
        state: String,
    },
}

type GpuRow = (u32, String, String, String, bool, bool);

fn print_gpu_table(rows: &[GpuRow]) {
    let mut id_w = 2usize;
    let mut name_w = 4usize;
    let mut pci_w = 3usize;
    let mut render_w = 6usize;

    for (id, name, pci, render, _, _) in rows {
        id_w = id_w.max(id.to_string().len());
        name_w = name_w.max(name.len());
        pci_w = pci_w.max(pci.len());
        render_w = render_w.max(render.len());
    }

    println!(
        "{:<id_w$}  {:<name_w$}  {:<pci_w$}  {:<render_w$}  {:<7}  {:<7}",
        "ID",
        "NAME",
        "PCI",
        "RENDER",
        "DEFAULT",
        "BLOCKED",
        id_w = id_w,
        name_w = name_w,
        pci_w = pci_w,
        render_w = render_w,
    );
    println!(
        "{}  {}  {}  {}  {}  {}",
        "-".repeat(id_w),
        "-".repeat(name_w),
        "-".repeat(pci_w),
        "-".repeat(render_w),
        "-".repeat(7),
        "-".repeat(7),
    );

    for (id, name, pci, render, is_default, blocked) in rows {
        println!(
            "{:<id_w$}  {:<name_w$}  {:<pci_w$}  {:<render_w$}  {:<7}  {:<7}",
            id,
            name,
            pci,
            render,
            if *is_default { "yes" } else { "no" },
            if *blocked { "on*" } else { "off" },
            id_w = id_w,
            name_w = name_w,
            pci_w = pci_w,
            render_w = render_w,
        );
    }
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
            let mut response: Vec<GpuRow> = proxy.call("ListGpus", &()).await?;
            response.sort_by_key(|row| row.0);
            print_gpu_table(&response);
        }
        Commands::Gpu { id, command } => match command {
            GpuCommands::Block { state } => {
                let block = match state.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => {
                        return Err(format!(
                            "Invalid state '{}'. Expected: on or off",
                            state
                        )
                        .into())
                    }
                };
                let response: String = proxy.call("SetGpuBlock", &(id, block)).await?;
                println!("{}", response);
            }
        },
        Commands::ListModes => {
            let response: Vec<String> = proxy.call("ListMode", &()).await?;
            for mode in response {
                println!("{}", mode);
            }
        }
    }

    Ok(())
}
