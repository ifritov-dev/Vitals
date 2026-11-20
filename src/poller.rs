//! Reads the sensors on a worker thread.
//!
//! A poll is about sixty milliseconds of SMC calls. On the main thread that
//! lands inside the menu's event-tracking loop and the menu stutters while the
//! pointer moves, so the reads run here and the UI only ever applies a
//! finished snapshot.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use crate::store::{SensorStore, Snapshot};

pub struct Poller {
    requests: Sender<()>,
    results: Receiver<Snapshot>,
}

impl Poller {
    pub fn start() -> Poller {
        let (request_tx, request_rx) = channel::<()>();
        let (result_tx, result_rx) = channel::<Snapshot>();
        std::thread::spawn(move || {
            let mut store = SensorStore::new();
            while request_rx.recv().is_ok() {
                // Requests that arrived while this poll ran are already
                // answered by it; one poll covers them all.
                while matches!(request_rx.try_recv(), Ok(())) {}
                store.refresh();
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

    pub fn request(&self) {
        let _ = self.requests.send(());
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
