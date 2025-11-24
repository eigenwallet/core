mod parse;

use crate::cli::Cmd;
use clap::{CommandFactory, Parser};
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{error::ReadlineError, Editor};
use rustyline::{Context, Helper};
use std::future::Future;
use swap_controller_api::AsbApiClient;

struct CommandCompleter {
    commands: Vec<String>,
}

impl CommandCompleter {
    fn new() -> Self {
        let mut commands = Vec::new();

        // Extract command names from the Cmd enum using clap
        #[derive(Parser)]
        #[command(name = "")]
        #[command(about = "")]
        #[command(no_binary_name = true)]
        struct TempReplCommand {
            #[command(subcommand)]
            cmd: Cmd,
        }

        let app = TempReplCommand::command();
        for subcmd in app.get_subcommands() {
            commands.push(subcmd.get_name().to_string());
            // Also add aliases if any
            for alias in subcmd.get_all_aliases() {
                commands.push(alias.to_string());
            }
        }

        // Add shell-specific commands
        commands.extend_from_slice(&[
            "help".to_string(),
            "quit".to_string(),
            "exit".to_string(),
            ":q".to_string(),
        ]);

        commands.sort();

        Self { commands }
    }
}

impl Helper for CommandCompleter {}

impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let line = &line[..pos];
        let words: Vec<&str> = line.split_whitespace().collect();

        if words.is_empty() || (words.len() == 1 && !line.ends_with(' ')) {
            // Complete command names
            let start_pos = line.rfind(' ').map(|i| i + 1).unwrap_or(0);
            let prefix = &line[start_pos..];

            let matches: Vec<Pair> = self
                .commands
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| Pair {
                    display: cmd.clone(),
                    replacement: cmd.clone(),
                })
                .collect();

            Ok((start_pos, matches))
        } else {
            // No completion for command arguments yet
            Ok((pos, Vec::new()))
        }
    }
}

impl Hinter for CommandCompleter {
    type Hint = String;
}

impl Highlighter for CommandCompleter {}

impl Validator for CommandCompleter {}

pub async fn run<C, F, Fut>(client: C, dispatch: F) -> anyhow::Result<()>
where
    C: AsbApiClient + Clone + Send + 'static,
    F: Fn(Cmd, C) -> Fut + Clone + 'static,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let completer = CommandCompleter::new();
    let mut rl = Editor::new()?;
    rl.set_helper(Some(completer));

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
                                eprintln!("Command failed with error: {e:?}");
                            }
                        } else {
                            eprintln!(
                                "Unknown command: {line}. Type 'help' for available commands."
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
                eprintln!("Error: {err:?}");
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
