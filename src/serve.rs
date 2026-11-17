//! The one live process: memory, the door, and the event loop, which does one thing at a time.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use jiff::Timestamp;

use crate::event::{Due, Event, Request, Response};
use crate::home::Home;
use crate::{Error, Result, shipit};

/// The longest the loop sleeps between looks at the clock. It sleeps on a steady clock while events
/// are set on the wall clock, which can be moved; looking again at least this often bounds how
/// late a moved clock can make an event.
const LOOK_AGAIN: Duration = Duration::from_secs(60);

/// How long a client may take to send its line. The loop waits for nobody else meanwhile, so a
/// client that connects and says nothing must not hold it.
const CLIENT_SILENCE: Duration = Duration::from_secs(10);

/// Make the home if it is missing, hold it, open the door, and run the loop until a client asks
/// to stop.
pub fn serve(home: &Home) -> Result<()> {
    home.make()?;
    let _held = home.claim()?;
    let sock = home.sock();
    // holding the home, this serve alone may clear the door another left
    match std::fs::remove_file(&sock) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::File {
                what: "clearing",
                path: sock,
                source,
            });
        }
    }
    let door = UnixListener::bind(&sock).map_err(|source| Error::File {
        what: "opening",
        path: sock.clone(),
        source,
    })?;
    let mut events = home.load()?;

    let (knock, knocks) = mpsc::channel();
    thread::spawn(move || {
        for stream in door.incoming() {
            if knock.send(stream).is_err() {
                return; // the loop has ended
            }
        }
    });

    loop {
        settle(&mut events, home)?;
        let sleep = until_next(&events, Timestamp::now());
        match knocks.recv_timeout(sleep) {
            Ok(Ok(stream)) => {
                if answer(stream, &mut events, home) == Flow::Stop {
                    return Ok(());
                }
            }
            // a connection that failed before it was ours: the client sees it, serve goes on
            Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::Io {
                    what: "listening at the door",
                    source: std::io::Error::other("the thread at the door ended"),
                });
            }
        }
    }
}

/// Run each waiting event whose moment has come, in the order they were added, and expire each
/// one missed; memory is snapshotted after each change. A snapshot that fails ends serve: memory
/// and the store must not part, and a restart reads the store again.
fn settle(events: &mut [Event], home: &Home) -> Result<()> {
    for i in 0..events.len() {
        let due = match events.get(i) {
            Some(event) if !event.expired => event.due(Timestamp::now())?,
            Some(_) | None => continue,
        };
        let outcome = match (due, events.get(i)) {
            (Due::Wait, _) | (_, None) => continue,
            (Due::Missed, Some(_)) => None,
            (Due::Run, Some(event)) => Some(shipit::ship(event, home)),
        };
        if let Some(event) = events.get_mut(i) {
            event.expired = true;
            event.result = outcome;
        }
        home.save(events)?;
    }
    Ok(())
}

/// How long to sleep: until the next waiting event's moment, and no longer than `LOOK_AGAIN`.
fn until_next(events: &[Event], now: Timestamp) -> Duration {
    events
        .iter()
        .filter(|e| !e.expired)
        .map(|e| match Duration::try_from(e.at.duration_since(now)) {
            Ok(d) => d,
            // its moment passed while the loop was busy: look again now
            Err(_) => Duration::ZERO,
        })
        .fold(LOOK_AGAIN, Duration::min)
}

#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Go,
    Stop,
}

/// Read one request, do it, write one response. What goes wrong with the client's own connection
/// is the client's to see; serve goes on.
fn answer(stream: UnixStream, events: &mut Vec<Event>, home: &Home) -> Flow {
    let mut line = String::new();
    let read = stream
        .set_read_timeout(Some(CLIENT_SILENCE))
        .and_then(|()| BufReader::new(&stream).read_line(&mut line));
    let (response, flow) = match read {
        Ok(_) => match serde_json::from_str::<Request>(&line) {
            Ok(request) => respond(request, events, home),
            Err(e) => (Response::Err(format!("not a request: {e}")), Flow::Go),
        },
        Err(e) => (Response::Err(format!("reading the request: {e}")), Flow::Go),
    };
    let sent = serde_json::to_string(&response).map(|mut text| {
        text.push('\n');
        text
    });
    match sent.map(|text| (&stream).write_all(text.as_bytes())) {
        Ok(Ok(())) => {}
        // the client hung up before hearing back; it will ask again
        Ok(Err(_)) => {}
        // a response is a string or JSON already: it always serializes
        Err(_) => {}
    }
    flow
}

fn respond(request: Request, events: &mut Vec<Event>, home: &Home) -> (Response, Flow) {
    let done = match request {
        Request::Stop {} => return (Response::Ok(serde_json::Value::Null), Flow::Stop),
        Request::Add {
            label,
            at,
            retries,
            timeout,
            function,
        } => add(
            Event {
                id: match next_id(events) {
                    Ok(id) => id,
                    Err(e) => return (Response::Err(e.to_string()), Flow::Go),
                },
                label,
                at,
                retries,
                timeout,
                function: *function,
                expired: false,
                result: None,
            },
            events,
            home,
        ),
        Request::Get { id: Some(id), .. } => find(events, id).and_then(|i| json(&events[i])),
        Request::Get { id: None, expired } => {
            let picked: Vec<&Event> = events.iter().filter(|e| e.expired == expired).collect();
            serde_json::to_value(picked).map_err(Error::from)
        }
        Request::Del { id } => del(id, events, home),
    };
    match done {
        Ok(value) => (Response::Ok(value), Flow::Go),
        Err(e) => (Response::Err(e.to_string()), Flow::Go),
    }
}

/// Add an event: one whose moment is still to come, with tries to give it, and a function that
/// passes its own check. Memory takes it only if the snapshot does.
fn add(event: Event, events: &mut Vec<Event>, home: &Home) -> Result<serde_json::Value> {
    if event.at <= Timestamp::now() {
        return Err(Error::InvalidInput(format!("at {} has passed", event.at)));
    }
    if event.retries == 0 || event.timeout == 0 {
        return Err(Error::InvalidInput(
            "retries and timeout must be at least 1".into(),
        ));
    }
    event.last_moment()?;
    event.function.check()?;
    let value = serde_json::to_value(&event)?;
    events.push(event);
    if let Err(e) = home.save(events) {
        events.pop();
        return Err(e);
    }
    Ok(value)
}

/// Delete an event. Memory lets it go only if the snapshot does.
fn del(id: u64, events: &mut Vec<Event>, home: &Home) -> Result<serde_json::Value> {
    let i = find(events, id)?;
    let gone = events.remove(i);
    if let Err(e) = home.save(events) {
        events.insert(i, gone);
        return Err(e);
    }
    json(&gone)
}

fn find(events: &[Event], id: u64) -> Result<usize> {
    events
        .iter()
        .position(|e| e.id == id)
        .ok_or_else(|| Error::InvalidInput(format!("no event has id {id}")))
}

/// One past the largest id there is, or 1 for the first.
fn next_id(events: &[Event]) -> Result<u64> {
    match events.iter().map(|e| e.id).max() {
        Some(max) => max
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("every id is taken".into())),
        None => Ok(1),
    }
}

fn json(event: &Event) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(event)?)
}
