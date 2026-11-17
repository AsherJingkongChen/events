//! What an event does when its moment comes: a command for bash, and what goes into its stdin.
//!
//! The command holds nothing private. Keys and everything of the message go through stdin, so `ps`,
//! which anyone on the machine can run, shows only the command.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::event::Http;
use crate::{Error, Result};

/// The kinds of function, each named by its key: `{"pushover":{...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Function {
    Pushover(Pushover),
}

/// A message through Pushover, its fields named as <https://pushover.net/api> names them. The app
/// token and the user key are not here: they are read from `.env` each time a message is sent.
/// `encrypted` is not here either: it asks the sender to have encrypted the message, which this
/// does not do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pushover {
    pub message: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub url_title: Option<String>,
    pub priority: Option<i8>,
    pub sound: Option<String>,
    pub timestamp: Option<u64>,
    pub ttl: Option<u64>,
    pub device: Option<String>,
    pub html: Option<bool>,
    pub monospace: Option<bool>,
    pub retry: Option<u64>,
    pub expire: Option<u64>,
    pub callback: Option<String>,
    pub tags: Option<String>,
    pub attachment_base64: Option<String>,
    pub attachment_type: Option<String>,
}

/// Where Pushover takes messages.
const PUSHOVER_MESSAGES: &str = "https://api.pushover.net/1/messages.json";

/// The keys in `.env` that Pushover needs, and the fields they go to.
const PUSHOVER_KEYS: [Key; 2] = [
    Key {
        env: "PUSHOVER_API_TOKEN",
        field: "token",
    },
    Key {
        env: "PUSHOVER_USER_KEY",
        field: "user",
    },
];

struct Key {
    env: &'static str,
    field: &'static str,
}

impl Pushover {
    /// A message with nothing else set.
    pub fn new(message: &str) -> Pushover {
        Pushover {
            message: message.into(),
            title: None,
            url: None,
            url_title: None,
            priority: None,
            sound: None,
            timestamp: None,
            ttl: None,
            device: None,
            html: None,
            monospace: None,
            retry: None,
            expire: None,
            callback: None,
            tags: None,
            attachment_base64: None,
            attachment_type: None,
        }
    }

    /// Each field that is set, by its Pushover name, as Pushover takes it.
    fn fields(&self) -> Vec<(&'static str, String)> {
        let flag = |b: bool| if b { "1" } else { "0" }.to_string();
        let texts = [
            ("title", &self.title),
            ("url", &self.url),
            ("url_title", &self.url_title),
            ("sound", &self.sound),
            ("device", &self.device),
            ("callback", &self.callback),
            ("tags", &self.tags),
            ("attachment_base64", &self.attachment_base64),
            ("attachment_type", &self.attachment_type),
        ];
        let numbers = [
            ("timestamp", self.timestamp),
            ("ttl", self.ttl),
            ("retry", self.retry),
            ("expire", self.expire),
        ];
        let flags = [("html", self.html), ("monospace", self.monospace)];
        let mut fields = vec![("message", self.message.clone())];
        fields.extend(
            texts
                .into_iter()
                .filter_map(|(name, v)| v.clone().map(|v| (name, v))),
        );
        if let Some(p) = self.priority {
            fields.push(("priority", p.to_string()));
        }
        fields.extend(
            numbers
                .into_iter()
                .filter_map(|(name, v)| v.map(|v| (name, v.to_string()))),
        );
        fields.extend(
            flags
                .into_iter()
                .filter_map(|(name, v)| v.map(|v| (name, flag(v)))),
        );
        fields
    }
}

impl Function {
    pub fn from_json(json: &str) -> Result<Function> {
        Ok(serde_json::from_str(json)?)
    }

    /// The command bash runs. It holds no key and nothing of the message. It ends 0 only when the
    /// message went through: curl's own failure ends it with curl's code, and an HTTP status of
    /// 400 or more with 22, as `--fail-with-body` would (curl before 7.76 has no such flag). It
    /// prints the body, then the status on a line of its own.
    pub fn to_bash(&self) -> String {
        match self {
            Function::Pushover(_) => format!(
                "out=$(curl -sS -w '\\n%{{http_code}}' -K - {PUSHOVER_MESSAGES}) || exit $?; \
                 printf '%s' \"$out\"; \
                 [ \"${{out##*$'\\n'}}\" -lt 400 ] || exit 22"
            ),
        }
    }

    /// What goes into the command's stdin: here, curl's config lines, one `form-string` a field,
    /// the keys from `env` first. A key missing or empty is the reason the command is not run.
    pub fn to_stdin(&self, env: &HashMap<String, String>) -> Result<String, String> {
        match self {
            Function::Pushover(p) => {
                let mut lines = String::new();
                for key in &PUSHOVER_KEYS {
                    match env.get(key.env) {
                        Some(value) if !value.is_empty() => {
                            lines.push_str(&form_string(key.field, value));
                        }
                        Some(_) => return Err(format!("{} is empty in .env", key.env)),
                        None => return Err(format!("{} is not in .env", key.env)),
                    }
                }
                for (name, value) in p.fields() {
                    lines.push_str(&form_string(name, &value));
                }
                Ok(lines)
            }
        }
    }

    /// The HTTP reply in what the command printed, when it got one. The command prints the body,
    /// then a line with the status, which curl writes as 000 when no reply came.
    pub fn http(&self, stdout: &str) -> Option<Http> {
        match self {
            Function::Pushover(_) => {
                let (body, status) = stdout.rsplit_once('\n')?;
                match status.trim().parse::<u16>() {
                    Ok(0) | Err(_) => None,
                    Ok(status) => Some(Http {
                        status,
                        body: body.to_string(),
                    }),
                }
            }
        }
    }

    /// What the types cannot say, checked when the event is added: Pushover's emergency priority,
    /// 2, needs `retry` and `expire`. The rest Pushover checks, and says so with a 4xx.
    pub fn check(&self) -> Result<()> {
        match self {
            Function::Pushover(p) => match (p.priority, p.retry, p.expire) {
                (Some(2), Some(_), Some(_)) => Ok(()),
                (Some(2), _, _) => Err(Error::InvalidInput(
                    "pushover priority 2 needs retry and expire".into(),
                )),
                _ => Ok(()),
            },
        }
    }
}

/// One line of curl's config: `form-string = "name=value"`, the value escaped as curl reads a
/// quoted parameter, so a quote, a backslash or a newline in it stays itself.
fn form_string(name: &str, value: &str) -> String {
    let mut line = format!("form-string = \"{name}=");
    for c in value.chars() {
        match c {
            '\\' => line.push_str("\\\\"),
            '"' => line.push_str("\\\""),
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push_str("\\t"),
            '\u{b}' => line.push_str("\\v"),
            c => line.push(c),
        }
    }
    line.push_str("\"\n");
    line
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "in a test an unwrap that fails is the test failing"
)]
mod tests {
    use super::*;

    fn env() -> HashMap<String, String> {
        HashMap::from([
            (
                "PUSHOVER_API_TOKEN".to_string(),
                "tokentokentoken".to_string(),
            ),
            ("PUSHOVER_USER_KEY".to_string(), "useruseruser".to_string()),
        ])
    }

    #[test]
    fn the_command_holds_nothing_private() {
        let mut p = Pushover::new("the final is at 13:20");
        p.title = Some("image processing".into());
        let f = Function::Pushover(p);
        let bash = f.to_bash();
        for private in [
            "tokentokentoken",
            "useruseruser",
            "13:20",
            "image processing",
        ] {
            assert!(!bash.contains(private), "{private} is in {bash}");
            assert!(f.to_stdin(&env()).unwrap().contains(private));
        }
    }

    #[test]
    fn a_quote_a_backslash_and_a_newline_stay_themselves() {
        let f = Function::Pushover(Pushover::new("say \"hi\"\\\nbye"));
        let stdin = f.to_stdin(&env()).unwrap();
        assert!(stdin.contains("form-string = \"message=say \\\"hi\\\"\\\\\\nbye\"\n"));
    }
}
