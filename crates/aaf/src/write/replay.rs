//! Replaying the times and identifiers pyaaf2 handed out while it wrote a
//! file, so that the same file can be written again byte for byte.
//!
//! This is testing support, public so that every crate whose tests compare
//! a written file with one pyaaf2 wrote can share it, and so that the Python
//! bindings can offer the same comparison to their own tests. It is not part
//! of the supported interface.
//!
//! A generator script writes each fixture with pyaaf2 and records beside it,
//! in a `<name>.calls.tsv` sidecar, how it was set up and every time and
//! identifier pyaaf2 (and whatever drove it) asked for, in order. [`Replay`]
//! hands those same values back in the same order, and notes the first time
//! it is asked for something else, which [`Replay::finish`] then reports.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use super::sources::{Clock, IdSource};
use super::value::Timestamp;
use crate::Auid;

/// What a [`Replay`] shares between its clones: the values still to hand
/// out, and the first way the writer's requests went wrong, if any did.
#[derive(Debug, Default)]
struct State {
    calls: VecDeque<(String, String)>,
    error: Option<String>,
}

/// The values pyaaf2 handed out, as a fixture's sidecar lists them, handed
/// back in the same order.
///
/// A clone draws from the same sequence, so one replay can serve as both a
/// writer's clock and its identifier source.
///
/// Clocks and identifier sources cannot fail, so a request for something
/// other than the next value in the sidecar, or for one after the sidecar
/// has run out, is answered with a placeholder and remembered; the file
/// written then differs, and [`finish`](Self::finish) says why.
#[derive(Debug, Clone)]
pub struct Replay {
    name: String,
    state: Arc<Mutex<State>>,
}

impl Replay {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next value, which must be of `kind`, or `None` after noting why
    /// not.
    fn next(&self, kind: &str) -> Option<String> {
        let mut state = self.lock();
        let problem = match state.calls.pop_front() {
            None => format!(
                "{}: the writer asked for {kind} after pyaaf2 had stopped asking",
                self.name
            ),
            Some((k, value)) if k == kind => return Some(value),
            Some((k, _)) => format!(
                "{}: the writer asked for {kind} where pyaaf2 asked for {k}",
                self.name
            ),
        };
        state.error.get_or_insert(problem);
        None
    }

    /// Whether the writer asked for exactly what pyaaf2 did: every value, in
    /// order, and no more.
    ///
    /// # Errors
    ///
    /// Says what the writer first asked for that pyaaf2 did not, or how many
    /// values it left unasked for.
    pub fn finish(&self) -> Result<(), String> {
        let state = self.lock();
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        match state.calls.front() {
            None => Ok(()),
            Some(next) => Err(format!(
                "{}: pyaaf2 asked for {} more value(s) than the writer did, starting with {next:?}",
                self.name,
                state.calls.len(),
            )),
        }
    }

    /// Panics unless the writer asked for exactly what pyaaf2 did, as
    /// [`finish`](Self::finish) checks.
    ///
    /// # Panics
    ///
    /// With `finish`'s error.
    pub fn assert_used_up(&self) {
        if let Err(error) = self.finish() {
            panic!("{error}");
        }
    }
}

impl Clock for Replay {
    fn now(&mut self) -> Timestamp {
        let value = self.next("now");
        match value.as_deref().map(Timestamp::parse_iso) {
            Some(Some(time)) => time,
            Some(None) => {
                self.lock().error.get_or_insert(format!(
                    "{}: {value:?} in the sidecar is not an ISO time",
                    self.name
                ));
                Timestamp::from_unix(0)
            }
            None => Timestamp::from_unix(0),
        }
    }
}

impl IdSource for Replay {
    fn uuid4(&mut self) -> Auid {
        let value = self.next("uuid4");
        match value.as_deref().map(str::parse::<Auid>) {
            Some(Ok(id)) => id,
            Some(Err(_)) => {
                self.lock().error.get_or_insert(format!(
                    "{}: {value:?} in the sidecar is not a UUID",
                    self.name
                ));
                Auid::NIL
            }
            None => Auid::NIL,
        }
    }
}

/// What a sidecar says about how its fixture was written.
#[derive(Debug)]
pub struct Sidecar {
    /// The sector size the file was written with.
    pub sector_size: u32,
    /// The writer's options, by name, where the generator set any.
    pub options: Vec<(String, bool)>,
    /// The user the generator said was logged in, if it said.
    pub user: Option<String>,
    /// The times and identifiers, to be replayed.
    pub replay: Replay,
}

impl Sidecar {
    /// Reads the sidecar at `path`, for the fixture called `name`.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, or is not a sidecar; see
    /// [`parse`](Self::parse).
    pub fn read(name: &str, path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(name, &text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses a sidecar's text, for the fixture called `name`.
    ///
    /// Lines are tab-separated: `sector_size` and a size, `option`, a name
    /// and `true` or `false`, `user` and a name, and then `now` and `uuid4`
    /// lines, which are the calls to replay. Lines starting `#` are
    /// comments.
    ///
    /// # Errors
    ///
    /// Names the first line that is none of those.
    pub fn parse(name: &str, text: &str) -> Result<Self, String> {
        let mut sector_size = 4096;
        let mut options = Vec::new();
        let mut user = None;
        let mut calls = VecDeque::new();
        for (number, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let bad = || format!("line {} is not a sidecar line: {line:?}", number + 1);
            let (kind, value) = line.split_once('\t').ok_or_else(bad)?;
            match kind {
                "sector_size" => sector_size = value.parse().map_err(|_| bad())?,
                "option" => {
                    let (option, on) = value.split_once('\t').ok_or_else(bad)?;
                    options.push((option.to_owned(), on == "true"));
                }
                "user" => user = Some(value.to_owned()),
                "now" | "uuid4" => calls.push_back((kind.to_owned(), value.to_owned())),
                _ => return Err(bad()),
            }
        }
        Ok(Self {
            sector_size,
            options,
            user,
            replay: Replay {
                name: name.to_owned(),
                state: Arc::new(Mutex::new(State { calls, error: None })),
            },
        })
    }

    /// Whether the generator turned `option` on.
    #[must_use]
    pub fn option(&self, option: &str) -> bool {
        self.options.iter().any(|(o, on)| o == option && *on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIDECAR: &str = "# a comment\nsector_size\t512\noption\tcreate_edgecode\ttrue\n\
                           user\teditor\nnow\t2024-05-06T07:08:09\n\
                           uuid4\t5eed0001-0000-4000-8000-000000000001\n";

    #[test]
    fn a_sidecar_is_replayed_in_order() {
        let sidecar = Sidecar::parse("x", SIDECAR).unwrap();
        assert_eq!(sidecar.sector_size, 512);
        assert!(sidecar.option("create_edgecode"));
        assert!(!sidecar.option("use_empty_mob_ids"));
        assert_eq!(sidecar.user.as_deref(), Some("editor"));
        let mut replay = sidecar.replay.clone();
        assert!(sidecar.replay.finish().is_err(), "nothing asked for yet");
        assert_eq!(replay.now().to_string(), "2024-05-06T07:08:09");
        assert_eq!(
            replay.uuid4().to_string(),
            "5eed0001-0000-4000-8000-000000000001"
        );
        sidecar.replay.finish().unwrap();
    }

    #[test]
    fn asking_out_of_turn_is_reported_by_finish() {
        let sidecar = Sidecar::parse("x", SIDECAR).unwrap();
        let mut replay = sidecar.replay.clone();
        assert_eq!(replay.uuid4(), Auid::NIL);
        let error = sidecar.replay.finish().unwrap_err();
        assert!(
            error.contains("asked for uuid4 where pyaaf2 asked for now"),
            "{error}"
        );
    }

    #[test]
    fn a_line_that_is_not_a_sidecar_line_is_refused() {
        assert!(Sidecar::parse("x", "later\t1\n").is_err());
        assert!(Sidecar::parse("x", "no tab\n").is_err());
    }
}
