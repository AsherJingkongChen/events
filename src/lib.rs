//! Events: Claude plans your events. They happen.
//!
//! `events serve` is the one live process. It keeps every event in memory, runs each one's function
//! when its moment comes, and answers clients at one Unix socket, one JSON line in and one out. Its
//! event loop is synchronous: one thing at a time, and memory is written to `store.jsonl` before
//! anyone hears that it changed.

mod client;
mod error;
mod event;
mod function;
mod home;
mod serve;
mod shipit;

pub use client::ask;
pub use error::{Error, Result};
pub use event::{Body, Due, Event, Http, Outcome, Request, Response};
pub use function::{Function, Pushover};
pub use home::Home;
pub use serve::serve;
