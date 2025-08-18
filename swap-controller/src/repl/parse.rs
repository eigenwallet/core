use crate::cli::Cmd;
use clap::Parser;

/// A wrapper for parsing REPL commands using clap
#[derive(Parser)]
#[command(name = "")]
#[command(about = "")]
#[command(no_binary_name = true)]
struct ReplCommand {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Parse a line from the REPL into a command using clap's parser
pub fn parse_line(line: &str) -> Option<Cmd> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Split the line into arguments, preserving quoted strings
    let args = shell_words::split(line).ok()?;

    // Try to parse using clap
    match ReplCommand::try_parse_from(args) {
        Ok(repl_cmd) => Some(repl_cmd.cmd),
        Err(err) => {
            // Print clap's error message (it's quite good)
            eprintln!("{}", err);
            None
        }
    }
}
