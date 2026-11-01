//! Lazy, single-thread job runner used for cold metadata and completion work.

use std::any::Any;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use termin_core::WakeHandle;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Type-erased cold-work result. The main thread chooses the concrete type with
/// [`WorkerResult::downcast`]; results are never applied on the worker thread.
pub struct WorkerResult(Box<dyn Any + Send + 'static>);

impl WorkerResult {
    pub fn downcast<T: Any + Send + 'static>(self) -> Result<T, Self> {
        self.0.downcast::<T>().map(|value| *value).map_err(Self)
    }
}

enum Message {
    Run(Job),
    Stop,
}

struct State {
    sender: Option<mpsc::Sender<Message>>,
    thread: Option<JoinHandle<()>>,
    generation: u64,
    stopped: bool,
    results: mpsc::Receiver<WorkerResult>,
}

/// Cloneable entry point for submitting cold work.
#[derive(Clone)]
pub struct Worker {
    state: Arc<Mutex<State>>,
    wake: WakeHandle,
    idle_timeout: Duration,
    result_sender: mpsc::Sender<WorkerResult>,
}

impl Worker {
    pub fn new(wake: WakeHandle) -> Self {
        Self::with_idle_timeout(wake, Duration::from_secs(30))
    }

    fn with_idle_timeout(wake: WakeHandle, idle_timeout: Duration) -> Self {
        let (result_sender, results) = mpsc::channel();
        Self {
            state: Arc::new(Mutex::new(State {
                sender: None,
                thread: None,
                generation: 0,
                stopped: false,
                results,
            })),
            wake,
            idle_timeout,
            result_sender,
        }
    }

    /// Queue work whose value must be applied by the main thread. The worker
    /// publishes a result and wakes the app after the job completes.
    pub fn submit_result<T: Any + Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> bool {
        let sender = self.result_sender.clone();
        self.submit(move || {
            let value = job();
            let _ = sender.send(WorkerResult(Box::new(value)));
        })
    }

    /// Move all currently available results into `out` without blocking.
    pub fn drain_results(&self, out: &mut Vec<WorkerResult>) {
        let Ok(state) = self.state.lock() else { return };
        while let Ok(result) = state.results.try_recv() {
            out.push(result);
        }
    }

    /// Queue one job. The thread is created lazily and exits after 30 seconds idle.
    pub fn submit(&self, job: impl FnOnce() + Send + 'static) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.stopped {
            return false;
        }
        if state.sender.is_none()
            && !start(
                &mut state,
                self.state.clone(),
                self.wake.clone(),
                self.idle_timeout,
            )
        {
            return false;
        }
        let message = Message::Run(Box::new(job));
        let send_result = state.sender.as_ref().map(|sender| sender.send(message));
        match send_result {
            Some(Ok(())) => true,
            Some(Err(mpsc::SendError(Message::Run(job)))) => {
                // The worker exited unexpectedly (for example, a panicking job).
                // It has already dropped the receiver, so joining while holding
                // this mutex cannot block a worker-side state transition.
                state.sender = None;
                if let Some(thread) = state.thread.take() {
                    let _ = thread.join();
                }
                if !start(
                    &mut state,
                    self.state.clone(),
                    self.wake.clone(),
                    self.idle_timeout,
                ) {
                    return false;
                }
                state
                    .sender
                    .as_ref()
                    .is_some_and(|sender| sender.send(Message::Run(job)).is_ok())
            }
            Some(Err(mpsc::SendError(Message::Stop))) | None => false,
        }
    }

    /// Stop the worker and join it. Safe to call more than once.
    pub fn shutdown(&self) {
        let (sender, thread) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.stopped = true;
            (state.sender.take(), state.thread.take())
        };
        if let Some(sender) = sender {
            let _ = sender.send(Message::Stop);
        }
        if let Some(thread) = thread {
            if thread.thread().id() != thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) == 1 {
            self.shutdown();
        }
    }
}

fn run(
    receiver: mpsc::Receiver<Message>,
    wake: WakeHandle,
    state: Arc<Mutex<State>>,
    generation: u64,
    idle_timeout: Duration,
) {
    loop {
        match receiver.recv_timeout(idle_timeout) {
            Ok(Message::Run(job)) => {
                job();
                wake.wake();
            }
            Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Coordinate the idle transition with submit(). A submit either
                // queues before this lock and is received below, or sees no
                // sender and starts a new channel after this worker retires.
                let Ok(mut shared) = state.lock() else { break };
                if shared.generation != generation {
                    break;
                }
                match receiver.try_recv() {
                    Ok(Message::Run(job)) => {
                        drop(shared);
                        job();
                        wake.wake();
                    }
                    Ok(Message::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => {
                        shared.sender = None;
                        shared.thread = None;
                        break;
                    }
                }
            }
        }
    }
}

fn start(
    state: &mut State,
    shared: Arc<Mutex<State>>,
    wake: WakeHandle,
    idle_timeout: Duration,
) -> bool {
    let (sender, receiver) = mpsc::channel();
    state.generation = state.generation.wrapping_add(1);
    let generation = state.generation;
    let Ok(thread) = thread::Builder::new()
        .name("termin-worker".into())
        .spawn(move || run(receiver, wake, shared, generation, idle_timeout))
    else {
        return false;
    };
    state.sender = Some(sender);
    state.thread = Some(thread);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn submissions_racing_idle_exit_are_not_lost() {
        let count = Arc::new(AtomicUsize::new(0));
        let worker = Worker::with_idle_timeout(WakeHandle::new(|| {}), Duration::from_millis(8));
        for _ in 0..64 {
            let count = count.clone();
            assert!(worker.submit(move || {
                count.fetch_add(1, Ordering::SeqCst);
            }));
            thread::sleep(Duration::from_millis(9));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while count.load(Ordering::SeqCst) != 64 && std::time::Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(count.load(Ordering::SeqCst), 64);
        worker.shutdown();
    }

    #[test]
    fn shutdown_does_not_hold_lock_while_job_submits() {
        let worker = Worker::with_idle_timeout(WakeHandle::new(|| {}), Duration::from_secs(1));
        let nested = worker.clone();
        let (started_tx, started_rx) = mpsc::channel();
        assert!(worker.submit(move || {
            started_tx.send(()).unwrap();
            let _ = nested.submit(|| {});
        }));
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.shutdown();
        assert!(!worker.submit(|| {}));
    }

    #[test]
    fn result_jobs_publish_values_for_main_thread_drain() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = wakes.clone();
        let worker = Worker::with_idle_timeout(
            WakeHandle::new(move || {
                wake_count.fetch_add(1, Ordering::SeqCst);
            }),
            Duration::from_secs(1),
        );
        assert!(worker.submit_result(|| String::from("metadata")));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut results = Vec::new();
        while results.is_empty() && std::time::Instant::now() < deadline {
            worker.drain_results(&mut results);
            thread::yield_now();
        }
        assert_eq!(results.len(), 1);
        assert_eq!(
            results.pop().unwrap().downcast::<String>().ok().unwrap(),
            "metadata"
        );
        assert!(wakes.load(Ordering::SeqCst) > 0);
        worker.shutdown();
    }
}
