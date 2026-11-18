use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use events::{Event, Function, Home, Request, Result, Sort};
use jiff::Timestamp;

/// Claude plans your events. They happen.
///
/// An event, and what it is made of:
///
/// - Event: when it happens (--at), who added it (--label), how hard to try it (--retries,
///   --timeout). Once its moment has passed it is expired, and holds what its function got.
///   - Function: what happens at that moment. Today one kind: pushover, a message on a phone.
///     - Arguments: the function's fields, in JSON: {"pushover":{"title":"exam","message":"13:20"}}
///     - Result: what came back: {"error":null,"http":{"status":200,"body":{"json":{"status":1}}}}
///
/// The same, every part, as JSON in BNF. Times are RFC 3339 in UTC, timeout is in seconds. A
/// pushover field that is null is left out: it need not be in --function, and it is not printed.
///
///   event    ::= { id: uint, label: string, at: rfc3339, retries: uint, timeout: uint,
///                  function: function, expired: bool, result: result | null }
///   function ::= { pushover: pushover }
///   pushover ::= { message: string, title: string | null, url: string | null,
///                  url_title: string | null, priority: int | null, sound: string | null,
///                  timestamp: uint | null, ttl: uint | null, device: string | null,
///                  html: bool | null, monospace: bool | null, retry: uint | null,
///                  expire: uint | null, callback: string | null, tags: string | null,
///                  attachment_base64: string | null, attachment_type: string | null }
///   result   ::= { error: string | null, http: http | null }
///   http     ::= { status: uint, body: body }
///   body     ::= { json: json } | { text: string }
///
/// `events serve` is the one live process; every other command asks it at the door, serve.sock
/// in the home. The home is $EVENTS_HOME; if that is unset, $XDG_DATA_HOME/events; if that is
/// unset too, ~/.local/share/events. Answers are JSON, one event a line: read them with jq.
#[derive(Parser)]
#[command(
    version,
    verbatim_doc_comment,
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
    /// Add an event. Prints the event added.
    ///
    /// Its function and the function's arguments come by --function. For pushover: message,
    /// required; title, url, url_title, priority (-2 to 2), sound, timestamp, ttl, device, html,
    /// monospace, retry and expire (both needed with priority 2), callback, tags,
    /// attachment_base64, attachment_type: each named and meant as https://pushover.net/api says.
    /// The keys are not arguments: serve reads them from .env (see serve --help).
    Add {
        /// The function and its arguments, taken as curl's -d takes data: the JSON itself, @FILE
        /// for a file, or @- for stdin. Through @FILE or @- a message stays out of ps.
        #[arg(long, value_name = "JSON|@FILE|@-")]
        function: String,
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
    /// Print the events still waiting, one a line, in the order of their ids; with --expired, the
    /// ones whose moment has passed; with an id, that one. An expired event's result.http is the
    /// reply it got: a 2xx status is done, another failed; null is missed, and result.error says
    /// why. A result that is null itself is missed too: nothing was tried, all through its time.
    Get {
        /// The id of the one event to print, waiting or expired.
        id: Option<u64>,
        /// The expired events instead of the waiting ones.
        #[arg(long)]
        expired: bool,
        /// The order to print them in.
        #[arg(long, value_enum, default_value_t = Sort::Id)]
        sort: Sort,
        /// Print the first N at most, in that order.
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// One line an event: id | at | label: waiting, or expired, done / failed / missed.
        #[arg(long)]
        short: bool,
    },
    /// Delete an event. Prints the event deleted.
    Del {
        /// The id of the event to delete, waiting or expired.
        id: u64,
    },
    /// End serve.
    Stop,
    /// Be the one live process: run each event at its moment, answer at the door. Its event loop
    /// does one thing at a time: events at the same moment run one after another, in the order
    /// they were added, and while one is tried, for retries × timeout seconds at most, every
    /// command waits its turn.
    ///
    /// It makes the home if it is missing, and holds it: a second serve on the same home exits
    /// with an error. The home's .env holds the keys, which the user fills and serve reads each
    /// time it sends, so a change needs no restart: PUSHOVER_API_TOKEN, of an application made at
    /// https://pushover.net/apps/build; PUSHOVER_USER_KEY, from the Pushover dashboard.
    Serve,
    /// Put this version's skill, which tells Claude how to use events, under PATH: it writes
    /// PATH/events/SKILL.md, over one that is there, and prints the file written. For Claude Code,
    /// PATH is ~/.claude/skills.
    Skill {
        /// The folder of skills to put it in.
        path: PathBuf,
    },
}

/// The skill, part of the source: it is built into the binary, so the two are always one version.
const SKILL: &str = include_str!("../.claude/skills/events/SKILL.md");

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
        // written from the binary alone: it asks nothing of the home or of serve
        Command::Skill { path } => skill(&path),
        Command::Serve => events::serve(&home),
        Command::Add {
            function,
            at,
            label,
            retries,
            timeout,
        } => {
            let function = Function::from_json(&data(function)?)?;
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
        Command::Get {
            id,
            expired,
            sort,
            limit,
            short,
        } => {
            let got = events::ask(
                &home,
                &Request::Get {
                    id,
                    expired,
                    sort,
                    limit,
                },
            )?;
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

/// Write the skill to `path/events/SKILL.md`, making the folders on the way, and print where.
fn skill(path: &Path) -> Result<()> {
    let folder = path.join("events");
    std::fs::create_dir_all(&folder).map_err(|source| events::Error::File {
        what: "making",
        path: folder.clone(),
        source,
    })?;
    let file = folder.join("SKILL.md");
    std::fs::write(&file, SKILL).map_err(|source| events::Error::File {
        what: "writing",
        path: file.clone(),
        source,
    })?;
    say(&file.display().to_string())
}

/// Data as curl's -d takes it: `@-` is stdin, `@FILE` a file, anything else the data itself.
fn data(arg: String) -> Result<String> {
    match arg.strip_prefix('@') {
        Some("-") => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|source| events::Error::Io {
                    what: "reading the function from stdin",
                    source,
                })?;
            Ok(text)
        }
        Some(path) => std::fs::read_to_string(path).map_err(|source| events::Error::File {
            what: "reading the function from",
            path: path.into(),
            source,
        }),
        None => Ok(arg),
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
