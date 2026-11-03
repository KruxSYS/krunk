//! Background jobs: run slow work (Argon2 key derivation) on a worker
//! thread and continue on the UI thread with the result.

use super::Browser;
use crate::log::warning;
use std::any::Any;
use std::sync::mpsc;

/// `WM_APP` code: a background job finished.
pub(super) const APP_JOB: usize = 3;

pub(super) type JobResult = (u64, Box<dyn Any + Send>);
pub(super) type JobDone = Box<dyn FnOnce(&Browser, Box<dyn Any + Send>)>;

pub(super) struct Jobs {
    next: std::cell::Cell<u64>,
    pending: std::cell::RefCell<std::collections::HashMap<u64, JobDone>>,
    tx: mpsc::Sender<JobResult>,
    rx: mpsc::Receiver<JobResult>,
}

impl Jobs {
    pub fn new() -> Jobs {
        let (tx, rx) = mpsc::channel();
        Jobs { next: std::cell::Cell::new(1), pending: Default::default(), tx, rx }
    }
}

impl Browser {
    /// Runs `work` on a new thread; `done` runs on the UI thread afterwards.
    pub(super) fn spawn_job<R: Send + 'static>(
        &self,
        name: &str,
        work: impl FnOnce() -> R + Send + 'static,
        done: impl FnOnce(&Browser, R) + 'static,
    ) {
        let Some(window) = self.window() else { return };
        let id = self.jobs.next.get();
        self.jobs.next.set(id + 1);
        self.jobs.pending.borrow_mut().insert(
            id,
            Box::new(move |b, any| {
                if let Ok(r) = any.downcast::<R>() {
                    done(b, *r);
                }
            }),
        );
        let tx = self.jobs.tx.clone();
        let notifier = window.notifier();
        let spawned = std::thread::Builder::new().name(name.to_string()).spawn(move || {
            let r = work();
            let _ = tx.send((id, Box::new(r)));
            notifier.notify(APP_JOB);
        });
        if spawned.is_err() {
            warning!("could not start background job {name}");
            self.jobs.pending.borrow_mut().remove(&id);
        }
    }

    /// `WM_APP(APP_JOB)`: finish every completed job.
    pub(super) fn jobs_ready(&self) {
        while let Ok((id, result)) = self.jobs.rx.try_recv() {
            let done = self.jobs.pending.borrow_mut().remove(&id);
            if let Some(done) = done {
                done(self, result);
            }
        }
    }
}
