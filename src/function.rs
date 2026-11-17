//! What an event does when its moment comes. Each kind is sent from here, in Rust: no command is
//! run, so nothing of it shows in `ps`, and no tool's version on the machine can change it.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::event::{Body, Http, Outcome};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monospace: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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

    /// The name of every key in `.env` that some kind of function needs.
    pub fn env_keys() -> impl Iterator<Item = &'static str> {
        PUSHOVER_KEYS.iter().map(|key| key.env)
    }

    /// Send it, waiting at most `timeout` for the whole reply. `keys` are `.env`'s: one missing or
    /// empty is the reason nothing is sent, the error. What comes back, a reply or why none came,
    /// is the outcome.
    pub fn send(
        &self,
        keys: &HashMap<String, String>,
        timeout: Duration,
    ) -> Result<Outcome, String> {
        match self {
            Function::Pushover(p) => {
                let mut form = Vec::new();
                for key in &PUSHOVER_KEYS {
                    match keys.get(key.env) {
                        Some(value) if !value.is_empty() => form.push((key.field, value.clone())),
                        Some(_) => return Err(format!("{} is empty in .env", key.env)),
                        None => return Err(format!("{} is not in .env", key.env)),
                    }
                }
                form.extend(p.fields());
                let config = ureq::Agent::config_builder()
                    .timeout_global(Some(timeout))
                    // a 4xx is an answer to keep, not an error to raise
                    .http_status_as_error(false)
                    .build();
                let agent = ureq::Agent::new_with_config(config);
                Ok(match agent.post(PUSHOVER_MESSAGES).send_form(form) {
                    Ok(mut reply) => {
                        let status = reply.status().as_u16();
                        match reply.body_mut().read_to_string() {
                            Ok(body) => Outcome {
                                error: None,
                                http: Some(Http {
                                    status,
                                    body: Body::of(body),
                                }),
                            },
                            Err(e) => Outcome {
                                error: Some(format!("reading the reply failed: {e}")),
                                http: Some(Http {
                                    status,
                                    body: Body::Text(String::new()),
                                }),
                            },
                        }
                    }
                    Err(e) => Outcome {
                        error: Some(e.to_string()),
                        http: None,
                    },
                })
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
