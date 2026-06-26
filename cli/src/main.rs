//! `dos` — CLI management tool.

mod search;
mod ping;
mod pair;

use tracing_subscriber::{fmt, EnvFilter};

fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let cmd = args[1].as_str();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            match cmd {
                "search" => {
                    let query = if args.len() > 2 { args[2].clone() } else { String::new() };
                    if let Err(e) = search::run_search(query).await {
                        eprintln!("Search failed: {}", e);
                    }
                }
                "ping" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos ping <node_id>");
                    } else if let Err(e) = ping::run_ping(&args[2]).await {
                        eprintln!("Ping failed: {}", e);
                    }
                }
                "pair" => {
                    if args.len() < 3 {
                        eprintln!("Usage: dos pair <node_id>");
                    } else if let Err(e) = pair::run_pair(&args[2]).await {
                        eprintln!("Pair failed: {}", e);
                    }
                }
                _ => {
                    println!("Unknown command: {}", cmd);
                    print_help();
                }
            }
        });

    Ok(())
}

fn print_help() {
    println!("dos — Personal Distributed OS v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USAGE:");
    println!("  dos <COMMAND>");
    println!();
    println!("COMMANDS:");
    println!("  devices     List all known nodes");
    println!("  search      Search for a device");
    println!("  pair        Pair with a new device");
    println!("  tasks       Show task history");
    println!("  ping        Ping a node");
}
