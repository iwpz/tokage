use serde_json::json;
use tokage_cli::{Args, HELP};

#[tokio::main]
async fn main() {
    if let Some(code) = tokage_native::chat::run_reasonix_sandbox_helper() {
        std::process::exit(code);
    }
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let machine = raw.iter().any(|s| s == "--json");
    let result = async {
        let args = Args::parse(raw)?;
        if args.command.is_empty() || args.command == ["--help"] || args.command == ["help"] {
            if machine {
                println!("{}", json!({"help": HELP}));
            } else {
                println!("{HELP}");
            }
            return Ok(());
        }
        if args.command == ["--version"] {
            println!(
                "{}",
                if machine {
                    json!({"version": env!("CARGO_PKG_VERSION")}).to_string()
                } else {
                    format!("tokage {}", env!("CARGO_PKG_VERSION"))
                }
            );
            return Ok(());
        }
        let value = tokage_cli::dispatch(args).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
        Ok::<_, String>(())
    }
    .await;
    if let Err(message) = result {
        if machine {
            eprintln!("{}", json!({"error":message}));
        } else {
            eprintln!("tokage: {message}");
        }
        std::process::exit(1);
    }
}
