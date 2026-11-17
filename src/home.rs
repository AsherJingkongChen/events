//! The home: one folder holding the store, the keys and the door, and the kernel's lock on it that
//! makes one serve the only one.

use std::collections::HashMap;
use std::fs::{DirBuilder, File, OpenOptions, Permissions, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::event::Event;
use crate::function::Function;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    /// `$EVENTS_HOME`; else `$XDG_DATA_HOME/events`; else `$HOME/.local/share/events`.
    pub fn locate() -> Result<Home> {
        let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
        let dir = match (var("EVENTS_HOME"), var("XDG_DATA_HOME"), var("HOME")) {
            (Some(home), _, _) => PathBuf::from(home),
            (None, Some(data), _) => PathBuf::from(data).join("events"),
            (None, None, Some(home)) => PathBuf::from(home).join(".local/share/events"),
            (None, None, None) => return Err(Error::NoHome),
        };
        Ok(Home { dir })
    }

    /// Memory's snapshot: one event a line.
    pub fn store(&self) -> PathBuf {
        self.dir.join("store.jsonl")
    }

    /// The keys functions need, which only they read, each time they run.
    pub fn env(&self) -> PathBuf {
        self.dir.join(".env")
    }

    /// The door: the one way in to serve.
    pub fn sock(&self) -> PathBuf {
        self.dir.join("serve.sock")
    }

    /// Make the folder, the store and `.env` (600) where they are missing; what is in them stays
    /// as it is. The folder is made 700 even when it was there: it alone guards the door.
    pub fn make(&self) -> Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .map_err(|source| file("making", &self.dir, source))?;
        std::fs::set_permissions(&self.dir, Permissions::from_mode(0o700))
            .map_err(|source| file("guarding", &self.dir, source))?;
        make_new(&self.store(), "")?;
        // every key a function needs, its value left for the user to fill
        let keys: String = Function::env_keys()
            .map(|key| format!("{key}=\n"))
            .collect();
        make_new(&self.env(), &keys)
    }

    /// Hold the home: the kernel's lock on its folder, kept while the file lives and let go however
    /// the process ends. Another serve holding it is an error.
    pub fn claim(&self) -> Result<File> {
        let dir = File::open(&self.dir).map_err(|source| file("opening", &self.dir, source))?;
        match dir.try_lock() {
            Ok(()) => Ok(dir),
            Err(TryLockError::WouldBlock) => Err(Error::AlreadyServing(self.dir.clone())),
            Err(TryLockError::Error(source)) => Err(file("locking", &self.dir, source)),
        }
    }

    /// The events in the store, as the last snapshot left them. A line that does not read as an
    /// event is an error, not a line skipped.
    pub fn load(&self) -> Result<Vec<Event>> {
        let path = self.store();
        let text =
            std::fs::read_to_string(&path).map_err(|source| file("reading", &path, source))?;
        text.lines()
            .enumerate()
            .map(|(n, line)| {
                serde_json::from_str(line).map_err(|source| Error::Store {
                    path: path.clone(),
                    line: n + 1,
                    source,
                })
            })
            .collect()
    }

    /// Write memory to the store: all of it to a new file, which then takes the store's name.
    pub fn save(&self, events: &[Event]) -> Result<()> {
        let path = self.store();
        let mut tmp = NamedTempFile::new_in(&self.dir)
            .map_err(|source| file("snapshotting", &path, source))?;
        for event in events {
            let mut line = serde_json::to_string(event)?;
            line.push('\n');
            tmp.write_all(line.as_bytes())
                .map_err(|source| file("snapshotting", &path, source))?;
        }
        tmp.as_file()
            .sync_all()
            .map_err(|source| file("snapshotting", &path, source))?;
        tmp.persist(&path)
            .map_err(|e| file("snapshotting", &path, e.error))?;
        Ok(())
    }

    /// The keys in `.env`, read now and dropped by whoever asked when it is done with them.
    pub fn keys(&self) -> Result<HashMap<String, String>, String> {
        let path = self.env();
        // why it cannot be read goes into the event's result, which every client sees: a line
        // that does not parse may hold a key, so only where it is is told, never what it says
        let unread = |e: dotenvy::Error| match e {
            dotenvy::Error::Io(e) => format!("reading {}: {e}", path.display()),
            _ => format!("{} has a line that is not KEY=value", path.display()),
        };
        dotenvy::from_path_iter(&path)
            .map_err(unread)?
            .map(|pair| pair.map_err(unread))
            .collect()
    }
}

/// Make a file with `content`, readable by its owner only, unless it is there already.
fn make_new(path: &Path, content: &str) -> Result<()> {
    let made = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path);
    match made {
        Ok(mut f) => f
            .write_all(content.as_bytes())
            .map_err(|source| file("making", path, source)),
        // there already: what is there stays
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(file("making", path, source)),
    }
}

fn file(what: &'static str, path: &Path, source: io::Error) -> Error {
    Error::File {
        what,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "in a test an unwrap that fails is the test failing"
)]
mod tests {
    use super::*;

    #[test]
    fn a_second_serve_cannot_hold_the_home() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home {
            dir: dir.path().to_path_buf(),
        };
        home.make().unwrap();
        let held = home.claim().unwrap();
        assert!(matches!(home.claim(), Err(Error::AlreadyServing(_))));
        drop(held);
        assert!(home.claim().is_ok());
    }

    #[test]
    fn a_line_of_env_that_does_not_parse_is_never_told() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home {
            dir: dir.path().to_path_buf(),
        };
        home.make().unwrap();
        std::fs::write(home.env(), "PUSHOVER_API_TOKEN=the-token \"oops\n").unwrap();
        let why = home.keys().unwrap_err();
        assert!(!why.contains("the-token"), "{why}");
    }
}
