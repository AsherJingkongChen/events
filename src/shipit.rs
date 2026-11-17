//! An event's moment: its function tried until it settles, its tries run out, or its time does.

use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use wait_timeout::ChildExt;

use crate::event::{Event, Outcome};
use crate::function::Function;
use crate::home::Home;

/// How one try ended.
enum Try {
    /// Nothing more to try: it is done, or the fault is the request's (a 4xx, a key not set).
    Settled(Outcome),
    /// The fault was the environment's (a 5xx, no reply, a timeout): another try may go through.
    Again(Outcome),
}

/// Run the event's function: tries one `timeout` apart, at most `retries`, none starting after the
/// event's last moment. What the last try gave is the event's result.
pub fn ship(event: &Event, home: &Home) -> Outcome {
    let timeout = Duration::from_secs(event.timeout);
    let mut last = Outcome::not_run(format!("retries is {}: nothing was tried", event.retries));
    for n in 1..=event.retries {
        let began = Instant::now();
        match try_once(&event.function, home, timeout) {
            Try::Settled(outcome) => return outcome,
            Try::Again(outcome) => last = outcome,
        }
        if n == event.retries {
            break;
        }
        // the next try starts one timeout after this one did
        if let Some(rest) = timeout.checked_sub(began.elapsed()) {
            thread::sleep(rest);
        }
        match event.last_moment() {
            Ok(end) if Timestamp::now() <= end => {}
            // its time is over, or cannot be told: the last try stands
            Ok(_) | Err(_) => break,
        }
    }
    last
}

/// One try: bash runs the function's command in a process group of its own, fed its stdin, and is
/// killed, group and all, when `timeout` passes.
fn try_once(function: &Function, home: &Home, timeout: Duration) -> Try {
    let stdin = match home.keys().and_then(|keys| function.to_stdin(&keys)) {
        Ok(stdin) => stdin,
        Err(why) => return Try::Settled(Outcome::not_run(why)),
    };
    let spawned = Command::new("bash")
        .arg("-c")
        .arg(function.to_bash())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return Try::Settled(Outcome::not_run(format!("bash did not start: {e}"))),
    };
    let mut notes = Vec::new();
    // fed and drained by threads, so a command that reads or writes a lot cannot stall the wait
    let feeder = child.stdin.take().map(|mut pipe| {
        thread::spawn(move || pipe.write_all(stdin.as_bytes()).map(|()| Vec::new()))
    });
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);

    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => Some(status),
        Ok(None) => {
            notes.push(format!("killed after {} s, its timeout", timeout.as_secs()));
            kill(&mut child, &mut notes);
            None
        }
        Err(e) => {
            notes.push(format!("waiting for it failed: {e}"));
            kill(&mut child, &mut notes);
            None
        }
    };
    joined(feeder, "feeding stdin", &mut notes);
    let stdout = text(joined(stdout, "reading stdout", &mut notes));
    let stderr = text(joined(stderr, "reading stderr", &mut notes));
    let exit_code = match status {
        Some(status) => exit_code(status, &mut notes),
        None => None,
    };
    let outcome = Outcome {
        exit_code,
        error: if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        },
        stderr,
        http: function.http(&stdout),
    };
    let request_at_fault = matches!(&outcome.http, Some(h) if (400..500).contains(&h.status));
    if outcome.exit_code == Some(0) || request_at_fault {
        Try::Settled(outcome)
    } else {
        Try::Again(outcome)
    }
}

/// The exit code, or, for a command ended by a signal, none and a note of which.
fn exit_code(status: ExitStatus, notes: &mut Vec<String>) -> Option<i32> {
    match (status.code(), status.signal()) {
        (Some(code), _) => Some(code),
        (None, Some(signal)) => {
            notes.push(format!("killed by signal {signal}"));
            None
        }
        (None, None) => {
            notes.push(format!("ended without a code or a signal: {status}"));
            None
        }
    }
}

/// Kill the child's whole process group, which `process_group(0)` made its own, then reap it.
fn kill(child: &mut Child, notes: &mut Vec<String>) {
    match i32::try_from(child.id()) {
        Ok(pid) => {
            if let Err(e) = killpg(Pid::from_raw(pid), Signal::SIGKILL) {
                notes.push(format!("killing its process group failed: {e}"));
            }
        }
        Err(e) => notes.push(format!("its pid {} is no pid to signal: {e}", child.id())),
    }
    if let Err(e) = child.wait() {
        notes.push(format!("reaping it failed: {e}"));
    }
}

fn drain(mut pipe: impl Read + Send + 'static) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).map(|_| bytes)
    })
}

/// What a pipe's thread gave; what went wrong becomes a note, and nothing.
fn joined(
    handle: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    what: &str,
    notes: &mut Vec<String>,
) -> Vec<u8> {
    match handle.map(JoinHandle::join) {
        Some(Ok(Ok(bytes))) => bytes,
        Some(Ok(Err(e))) => {
            notes.push(format!("{what} failed: {e}"));
            Vec::new()
        }
        Some(Err(_)) => {
            notes.push(format!("{what} panicked"));
            Vec::new()
        }
        None => {
            notes.push(format!("{what}: there was no pipe"));
            Vec::new()
        }
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}
