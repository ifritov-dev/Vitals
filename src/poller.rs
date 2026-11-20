//! Reads the sensors on a worker thread.
//!
//! A poll is about sixty milliseconds of SMC calls. On the main thread that
//! lands inside the menu's event-tracking loop and the menu stutters while the
//! pointer moves, so the reads run here and the UI only ever applies a
//! finished snapshot.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use crate::model::Category;
use crate::store::{SensorStore, Snapshot};

/// What a poll should cover: everything, or just the categories on screen.
enum Request {
    Full,
    Shown(Vec<(Category, Option<String>)>),
}

pub struct Poller {
    requests: Sender<Request>,
    results: Receiver<Snapshot>,
}

impl Poller {
    pub fn start() -> Poller {
        let (request_tx, request_rx) = channel::<Request>();
        let (result_tx, result_rx) = channel::<Snapshot>();
        std::thread::spawn(move || {
            let mut store = SensorStore::new();
            while let Ok(first) = request_rx.recv() {
                let mut latest = first;
                // Requests that arrived while this poll ran are already
                // answered by it; one poll covers them all.
                while let Ok(next) = request_rx.try_recv() {
                    latest = next;
                }
                match latest {
                    Request::Full => store.refresh(),
                    Request::Shown(shown) => store.refresh_shown(&shown),
                }
                if result_tx.send(store.snapshot()).is_err() {
                    break;
                }
            }
        });
        Poller {
            requests: request_tx,
            results: result_rx,
        }
    }

    /// Asks for a poll. `shown` is ignored when `full` is set.
    pub fn request(&self, full: bool, shown: &[(Category, Option<String>)]) {
        let request = if full {
            Request::Full
        } else {
            Request::Shown(shown.to_vec())
        };
        let _ = self.requests.send(request);
    }

    /// The newest finished poll, if one landed since the last call.
    pub fn take_latest(&self) -> Option<Snapshot> {
        let mut latest = None;
        loop {
            match self.results.try_recv() {
                Ok(snapshot) => latest = Some(snapshot),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        latest
    }
}
