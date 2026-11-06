//! A small pool for background work: jobs run in the order queued, at most
//! `size` at once, on threads started on demand (the TypeScript `Semaphore`).

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    workers: usize,
    running: usize,
}

pub struct Jobs {
    size: usize,
    state: Mutex<State>,
    changed: Condvar,
}

impl Jobs {
    pub fn new(size: usize) -> Self {
        Jobs { size: size.max(1), state: Mutex::default(), changed: Condvar::new() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn push(self: &Arc<Self>, job: impl FnOnce() + Send + 'static) {
        let mut state = self.lock();
        state.queue.push_back(Box::new(job));
        if state.workers < self.size {
            state.workers += 1;
            let jobs = self.clone();
            let spawned = std::thread::Builder::new().name("notebook-job".into()).spawn(move || jobs.work());
            if spawned.is_err() {
                state.workers -= 1;
            }
        }
    }

    fn work(&self) {
        loop {
            let job = {
                let mut state = self.lock();
                match state.queue.pop_front() {
                    Some(job) => {
                        state.running += 1;
                        job
                    }
                    None => {
                        state.workers -= 1;
                        self.changed.notify_all();
                        return;
                    }
                }
            };
            // A panicking job must not wedge the pool or `wait_idle`.
            let _ = catch_unwind(AssertUnwindSafe(job));
            self.lock().running -= 1;
            self.changed.notify_all();
        }
    }

    /// Blocks until the queue is empty and nothing is running.
    pub fn wait_idle(&self) {
        let mut state = self.lock();
        while !state.queue.is_empty() || state.running > 0 {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn runs_everything_at_most_size_at_once() {
        let jobs = Arc::new(Jobs::new(2));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..6 {
            let (in_flight, peak, done) = (in_flight.clone(), peak.clone(), done.clone());
            jobs.push(move || {
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                in_flight.fetch_sub(1, Ordering::SeqCst);
                done.fetch_add(1, Ordering::SeqCst);
            });
        }
        jobs.wait_idle();
        assert_eq!(done.load(Ordering::SeqCst), 6);
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_panicking_job_does_not_block_the_rest() {
        let jobs = Arc::new(Jobs::new(1));
        let done = Arc::new(AtomicUsize::new(0));
        jobs.push(|| panic!("boom"));
        let d = done.clone();
        jobs.push(move || {
            d.fetch_add(1, Ordering::SeqCst);
        });
        jobs.wait_idle();
        assert_eq!(done.load(Ordering::SeqCst), 1);
    }
}
