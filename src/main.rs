use std::io::{Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use events::{Event, Function, Home, Request, Result};
use jiff::Timestamp;

/// Claude plans your events. They happen.
///
/// `events serve` is the one live process; every other command asks it at the door,
/// `serve.sock` in the home (`$EVENTS_HOME`, else `$XDG_DATA_HOME/events`, else
/// `~/.local/share/events`). Answers are JSON, one event a line: read them with jq.
#[derive(Parser)]
#[command(
    version,
    after_help = concat!(
        "Something wrong, or missing? Tell your user, and with their word open an issue: ",
        env!("CARGO_PKG_REPOSITORY"),
        "/issues"
    )
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add an event. Its function is JSON on stdin, such as
    /// {"pushover":{"title":"exam","message":"13:20, bring a calculator"}}; the fields of
    /// pushover are named as https://pushover.net/api names them. Prints the event added.
    Add {
        /// When to run it, RFC 3339: 2026-11-24T11:30:00+08:00 or 2026-11-24T03:30:00Z. It must
        /// be still to come.
        #[arg(long)]
        at: Timestamp,
        /// Who adds it.
        #[arg(long)]
        label: String,
        /// How many times to try it at most.
        #[arg(long, default_value_t = 3)]
        retries: u32,
        /// Seconds one try may take; tries start this far apart.
        #[arg(long, value_name = "SECONDS", default_value_t = 30)]
        timeout: u64,
    },
    /// Print the events still waiting, one a line; with --expired, the ones whose moment has
    /// passed; with an id, that one. An expired event has a result unless it was missed: its
    /// exit_code is 0 when it was done, null when it never ran or was killed (error says why).
    Get {
        id: Option<u64>,
        /// The expired events instead of the waiting ones.
        #[arg(long)]
        expired: bool,
        /// One line an event: id | at | label: waiting, or expired, done / failed / missed.
        #[arg(long)]
        short: bool,
    },
    /// Delete an event. Prints the event deleted.
    Del { id: u64 },
    /// End serve.
    Stop,
    /// Be the one live process: run each event at its moment, answer at the door. Its event loop
    /// does one thing at a time, so events at the same moment run one after another.
    Serve,
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("events: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<()> {
    let home = Home::locate()?;
    match command {
        Command::Serve => events::serve(&home),
        Command::Add {
            at,
            label,
            retries,
            timeout,
        } => {
            let mut json = String::new();
            std::io::stdin()
                .read_to_string(&mut json)
                .map_err(|source| events::Error::Io {
                    what: "reading the function from stdin",
                    source,
                })?;
            let function = Function::from_json(&json)?;
            let added = events::ask(
                &home,
                &Request::Add {
                    label,
                    at,
                    retries,
                    timeout,
                    function: Box::new(function),
                },
            )?;
            say(&line(serde_json::from_value(added)?)?)
        }
        Command::Get { id, expired, short } => {
            let got = events::ask(&home, &Request::Get { id, expired })?;
            let got: Vec<Event> = match got {
                serde_json::Value::Array(_) => serde_json::from_value(got)?,
                one => vec![serde_json::from_value(one)?],
            };
            for event in got {
                if short {
                    say(&event.short())?;
                } else {
                    say(&line(event)?)?;
                }
            }
            Ok(())
        }
        Command::Del { id } => {
            let gone = events::ask(&home, &Request::Del { id })?;
            say(&line(serde_json::from_value(gone)?)?)
        }
        Command::Stop => {
            events::ask(&home, &Request::Stop {})?;
            Ok(())
        }
    }
}

/// An event as one compact JSON line, its fields in the order the event declares them.
fn line(event: Event) -> Result<String> {
    Ok(serde_json::to_string(&event)?)
}

/// Print a line. A reader that went away (`events get | head -1`) is not an error.
fn say(text: &str) -> Result<()> {
    match writeln!(std::io::stdout().lock(), "{text}") {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(source) => Err(events::Error::Io {
            what: "printing",
            source,
        }),
    }
}
