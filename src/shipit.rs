//! An event's moment: its function tried until it settles, its tries run out, or its time does.

use std::thread;
use std::time::{Duration, Instant};

use jiff::Timestamp;

use crate::event::{Event, Outcome};
use crate::home::Home;

/// Send the event's function: tries one `timeout` apart, at most `retries`, none starting after the
/// event's last moment. What the last try gave is the event's result.
pub fn ship(event: &Event, home: &Home) -> Outcome {
    let timeout = Duration::from_secs(event.timeout);
    let mut last = Outcome::not_run(format!("retries is {}: nothing was tried", event.retries));
    for n in 1..=event.retries {
        let began = Instant::now();
        // the keys are read for this try and dropped with it
        let sent = home
            .keys()
            .and_then(|keys| event.function.send(&keys, timeout));
        let outcome = match sent {
            Ok(outcome) => outcome,
            // nothing was sent, and another try would not change why
            Err(why) => return Outcome::not_run(why),
        };
        if settles(&outcome) {
            return outcome;
        }
        last = outcome;
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

/// A reply below 500 settles it: done, or the request's fault, which another try would not mend. A
/// 5xx, or no reply at all, is the environment's: another try may go through.
fn settles(outcome: &Outcome) -> bool {
    matches!(&outcome.http, Some(h) if h.status < 500)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Http;

    #[test]
    fn a_reply_below_500_settles_and_anything_else_is_tried_again() {
        let reply = |status| Outcome {
            error: None,
            http: Some(Http {
                status,
                body: String::new(),
            }),
        };
        assert!(settles(&reply(200)));
        assert!(settles(&reply(400)));
        assert!(settles(&reply(429)));
        assert!(!settles(&reply(500)));
        assert!(!settles(&reply(503)));
        assert!(!settles(&Outcome {
            error: Some("timed out".into()),
            http: None,
        }));
    }
}
