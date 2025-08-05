mod parse;

use crate::cli::Cmd;
use rustyline::{error::ReadlineError, DefaultEditor};
use std::future::Future;
use swap_controller_api::AsbApiClient;

pub async fn run<C, F, Fut>(client: C, dispatch: F) -> anyhow::Result<()>
where
    C: AsbApiClient + Clone + Send + 'static,
    F: Fn(Cmd, C) -> Fut + Clone + 'static,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut rl = DefaultEditor::new()?;

    println!("ASB Control Shell - Type 'help' for commands, 'quit' to exit\n");

    loop {
        let readline = rl.readline("asb> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;

                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                match line {
                    "quit" | "exit" | ":q" => {
                        println!("Goodbye!");
                        break;
                    }
                    "help" => {
                        print_help();
                    }
                    _ => {
                        if let Some(cmd) = parse::parse_line(line) {
                            if let Err(e) = dispatch(cmd, client.clone()).await {
                                eprintln!("Command failed: {}", e);
                            }
                        } else {
                            eprintln!(
                                "Unknown command: {}. Type 'help' for available commands.",
                                line
                            );
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}

fn print_help() {
    use crate::cli::Cmd;
    use clap::{CommandFactory, Parser};

    #[derive(Parser)]
    #[command(name = "")]
    #[command(about = "")]
    #[command(no_binary_name = true)]
    struct ReplCommand {
        #[command(subcommand)]
        cmd: Cmd,
    }

    println!("Available commands:");
    println!("{}", ReplCommand::command().render_help());
    println!("\nAdditional shell commands:");
    println!("  help                 Show this help message");
    println!("  quit, exit, :q       Exit the shell");
}
