//! The one live process: memory, the door, and the event loop, which does one thing at a time.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use jiff::Timestamp;

use crate::event::{Due, Event, Request, Response, Sort};
use crate::home::Home;
use crate::{Error, Result, shipit};

/// How long the loop waits on a client: this long for its line, and this long for it to take the
/// answer. A client of ours sends its line the moment it connects (55 µs after, measured) and
/// reads until it has its answer, so this is not a time any of them needs. The loop waits for
/// nobody else meanwhile: it is how long a client that stopped half-way can hold the loop, and so
/// how late one can make an event.
const CLIENT_WAIT: Duration = Duration::from_secs(10);

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
        let knocked = match until_next(&events, Timestamp::now())? {
            Some(sleep) => knocks.recv_timeout(sleep),
            // nothing waits: there is nothing to be late for, until someone knocks
            None => knocks.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match knocked {
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

/// Run each waiting event whose moment has come, in the order of their ids, and expire each one
/// missed; memory is snapshotted after each change. A snapshot that fails ends serve: memory and
/// the store must not part, and a restart reads the store again.
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

/// How long to sleep: until the next waiting event's moment, and no longer than any waiting
/// event's window. None when nothing waits.
///
/// The loop sleeps on a steady clock, which stands still while the machine is suspended; events
/// are set on the wall clock, which goes on, and can be moved. An event says how late it may run,
/// its window: looking at the clock again within it keeps the loop's own sleep from being why an
/// event is missed.
fn until_next(events: &[Event], now: Timestamp) -> Result<Option<Duration>> {
    let mut sleep = None;
    for event in events.iter().filter(|e| !e.expired) {
        let until = match Duration::try_from(event.at.duration_since(now)) {
            Ok(d) => d,
            // its moment passed while the loop was busy: look again now
            Err(_) => Duration::ZERO,
        };
        let look = until.min(event.window()?);
        sleep = match sleep {
            Some(sooner) => Some(look.min(sooner)),
            None => Some(look),
        };
    }
    Ok(sleep)
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
        .set_read_timeout(Some(CLIENT_WAIT))
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
    match sent.map(|text| write_within(&stream, text.as_bytes(), CLIENT_WAIT)) {
        Ok(Ok(())) => {}
        // the client hung up, or would not take its answer: what was asked is done all the same,
        // and `get` shows it
        Ok(Err(_)) => {}
        // a response is a string or JSON already: it always serializes
        Err(_) => {}
    }
    flow
}

/// Write all of `bytes`, in `limit` at most from the first byte to the last. A timeout on the
/// socket alone bounds each write, not their sum: a write that sent a part before its time ran
/// out counts as done, and the next is given the whole time again.
fn write_within(mut stream: &UnixStream, bytes: &[u8], limit: Duration) -> std::io::Result<()> {
    let began = Instant::now();
    let mut rest = bytes;
    while !rest.is_empty() {
        let left = match limit.checked_sub(began.elapsed()) {
            Some(left) if !left.is_zero() => left,
            Some(_) | None => return Err(std::io::ErrorKind::TimedOut.into()),
        };
        stream.set_write_timeout(Some(left))?;
        let sent = match stream.write(rest) {
            Ok(sent) => sent,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        rest = match rest.get(sent..) {
            Some(rest) if sent > 0 => rest,
            Some(_) | None => return Err(std::io::ErrorKind::WriteZero.into()),
        };
    }
    Ok(())
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
        Request::Get {
            id: None,
            expired,
            sort,
            limit,
        } => {
            let mut picked: Vec<&Event> = events.iter().filter(|e| e.expired == expired).collect();
            // memory is in id order already; the other orders fall back on it for ties
            match sort {
                Sort::Id => {}
                Sort::At => picked.sort_by_key(|e| e.at),
                Sort::Label => picked.sort_by(|a, b| a.label.cmp(&b.label)),
            }
            if let Some(limit) = limit {
                picked.truncate(limit);
            }
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
    // memory and the store are rows in the order of their ids: a new row goes last
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

/// The id after the last row's, or 1 for the first: rows are in the order of their ids, so the
/// last row holds the id given before this one.
fn next_id(events: &[Event]) -> Result<u64> {
    match events.last() {
        Some(prev) => prev
            .id
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("every id is taken".into())),
        None => Ok(1),
    }
}

fn json(event: &Event) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(event)?)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "in a test an unwrap that fails is the test failing"
)]
mod tests {
    use jiff::SignedDuration;

    use super::*;
    use crate::function::{Function, Pushover};

    #[test]
    fn the_loop_sleeps_to_the_next_moment_and_no_longer_than_a_window() {
        let now: Timestamp = "2026-11-24T03:00:00Z".parse().unwrap();
        let event = |ahead: i64, expired| Event {
            id: 1,
            label: "kiku".into(),
            at: now.checked_add(SignedDuration::from_secs(ahead)).unwrap(),
            retries: 3,
            timeout: 30,
            function: Function::Pushover(Pushover::new("13:20")),
            expired,
            result: None,
        };
        let sleep = |events: &[Event]| until_next(events, now).unwrap();
        assert_eq!(sleep(&[]), None);
        assert_eq!(sleep(&[event(-3600, true)]), None);
        assert_eq!(sleep(&[event(20, false)]), Some(Duration::from_secs(20)));
        // far off: the loop still looks at the clock once in its window, 3 tries of 30 s
        assert_eq!(sleep(&[event(3600, false)]), Some(Duration::from_secs(90)));
        assert_eq!(sleep(&[event(-5, false)]), Some(Duration::ZERO));
    }
}
