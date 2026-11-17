//! An event, and the packets that clients and serve trade at the door.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::function::Function;
use crate::{Error, Result};

/// Something to happen at a moment: a function to run then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Given by serve when the event is added.
    pub id: u64,
    /// Who added it.
    pub label: String,
    /// The moment to run it.
    pub at: Timestamp,
    /// How many times it is tried at most.
    pub retries: u32,
    /// Seconds one try may take, and the seconds from one try to the next.
    pub timeout: u64,
    pub function: Function,
    /// Its moment has passed: it ran, or it was missed. Nothing runs it again.
    pub expired: bool,
    /// What its last try gave; null while it waits, and for an event missed without running.
    pub result: Option<Outcome>,
}

/// What a try gave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// Null when the command did not end by itself: it never started, or it was killed.
    pub exit_code: Option<i32>,
    /// What serve says when the command could not: why it never started, or why it was killed.
    pub error: Option<String>,
    /// What the command said on stderr.
    pub stderr: String,
    /// The HTTP reply, when the function speaks HTTP and got one.
    pub http: Option<Http>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    pub status: u16,
    pub body: String,
}

/// Where a waiting event stands at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// Its moment has not come.
    Wait,
    /// Its moment has come, and there is time left for its tries.
    Run,
    /// Its moment and all its tries' time have passed: run now it would be late, which is useless.
    Missed,
}

impl Outcome {
    /// A try whose command never started, and why.
    pub fn not_run(why: String) -> Outcome {
        Outcome {
            exit_code: None,
            error: Some(why),
            stderr: String::new(),
            http: None,
        }
    }
}

impl Event {
    /// The last moment a try may start: `at`, and then `retries` tries of `timeout` seconds each.
    pub fn last_moment(&self) -> Result<Timestamp> {
        let too_long = || {
            Error::InvalidInput(format!(
                "{} tries of {} s from {} reach past any time there is",
                self.retries, self.timeout, self.at
            ))
        };
        let seconds = self
            .timeout
            .checked_mul(u64::from(self.retries))
            .ok_or_else(too_long)?;
        let seconds = i64::try_from(seconds).map_err(|_| too_long())?;
        self.at
            .checked_add(SignedDuration::from_secs(seconds))
            .map_err(|_| too_long())
    }

    /// Where this event stands at `now`, while it waits.
    pub fn due(&self, now: Timestamp) -> Result<Due> {
        if now < self.at {
            return Ok(Due::Wait);
        }
        if now <= self.last_moment()? {
            Ok(Due::Run)
        } else {
            Ok(Due::Missed)
        }
    }

    /// One line to glance at: `3 | 2026-11-24T03:30:00Z | kiku: waiting`.
    pub fn short(&self) -> String {
        let state = match (self.expired, &self.result) {
            (false, _) => "waiting",
            (true, None) => "expired, missed",
            (true, Some(outcome)) => match outcome.exit_code {
                Some(0) => "expired, done",
                None => "expired, missed",
                Some(_) => "expired, failed",
            },
        };
        format!("{} | {} | {}: {}", self.id, self.at, self.label, state)
    }
}

/// What a client asks serve, one JSON line: `{"add":{...}}`, `{"get":{...}}`, `{"del":{...}}`,
/// `{"stop":{}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// A new event; serve answers with it, its id given.
    Add {
        label: String,
        at: Timestamp,
        retries: u32,
        timeout: u64,
        function: Box<Function>,
    },
    /// The event of `id`; or, with no id, the events expired or not as `expired` says.
    Get { id: Option<u64>, expired: bool },
    /// Delete the event of `id`; serve answers with it.
    Del { id: u64 },
    /// End serve.
    Stop {},
}

/// What serve answers, one JSON line: `{"ok":...}` or `{"err":"..."}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Ok(serde_json::Value),
    Err(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::Pushover;

    #[test]
    #[expect(
        clippy::unwrap_used,
        reason = "in a test an unwrap that fails is the test failing"
    )]
    fn an_event_waits_runs_then_is_missed() {
        let at: Timestamp = "2026-11-24T03:30:00Z".parse().unwrap();
        let event = Event {
            id: 1,
            label: "kiku".into(),
            at,
            retries: 3,
            timeout: 30,
            function: Function::Pushover(Pushover::new("13:20")),
            expired: false,
            result: None,
        };
        let after = |seconds: i64| at.checked_add(SignedDuration::from_secs(seconds)).unwrap();
        assert_eq!(event.due(after(-1)).unwrap(), Due::Wait);
        assert_eq!(event.due(at).unwrap(), Due::Run);
        assert_eq!(event.due(after(89)).unwrap(), Due::Run);
        assert_eq!(event.due(after(90)).unwrap(), Due::Run);
        assert_eq!(event.due(after(91)).unwrap(), Due::Missed);
    }
}
