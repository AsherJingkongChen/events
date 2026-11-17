//! A client's side of the door: one request out, one response back.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use crate::event::{Request, Response};
use crate::home::Home;
use crate::{Error, Result};

/// Ask serve, and hand back what it answered: what it gave, or why it would not.
pub fn ask(home: &Home, request: &Request) -> Result<serde_json::Value> {
    let path = home.sock();
    let stream = UnixStream::connect(&path).map_err(|source| Error::NotServing {
        path: path.clone(),
        source,
    })?;
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|source| Error::Io {
            what: "sending the request",
            source,
        })?;
    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .map_err(|source| Error::Io {
            what: "reading the response",
            source,
        })?;
    match serde_json::from_str(&reply)? {
        Response::Ok(value) => Ok(value),
        Response::Err(why) => Err(Error::Refused(why)),
    }
}
