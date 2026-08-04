//! Interactive progress reporting.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::style::stderr_is_tty;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct Progress {
    message: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    stderr_is_tty: bool,
}

impl Progress {
    pub fn new() -> Progress {
        Progress {
            message: Arc::new(Mutex::new(String::new())),
            stop: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
            stderr_is_tty: stderr_is_tty(),
        }
    }

    /// Starts the interactive spinner once.
    pub fn start(&self) {
        if !self.stderr_is_tty {
            return;
        }

        let mut slot = lock(&self.thread);
        if slot.is_some() {
            return;
        }

        self.stop.store(false, Ordering::Release);
        let message = Arc::clone(&self.message);
        let stop = Arc::clone(&self.stop);
        if let Ok(handle) = thread::Builder::new().spawn(move || {
            let mut frame = 0;
            let mut current = String::new();
            while !stop.load(Ordering::Acquire) {
                let shared = lock(&message);
                current.clear();
                current.push_str(&shared);
                drop(shared);
                if !current.is_empty() {
                    let mut stderr = io::stderr().lock();
                    let _ = write!(
                        stderr,
                        "\r{} {}\x1b[K",
                        FRAMES[frame % FRAMES.len()],
                        current
                    );
                    let _ = stderr.flush();
                }
                frame += 1;
                thread::park_timeout(Duration::from_millis(100));
            }
        }) {
            *slot = Some(handle);
        }
    }

    /// Replaces the current status or emits one line on non-interactive stderr.
    pub fn set(&self, msg: &str) {
        if self.stderr_is_tty {
            if lock(&self.thread).is_some() {
                let mut message = lock(&self.message);
                message.clear();
                message.push_str(msg);
            }
        } else {
            eprintln!("zc: {msg}");
        }
    }

    /// Stops the spinner and erases its terminal line.
    pub fn clear(&self) {
        self.stop_thread();
        if self.stderr_is_tty {
            let mut stderr = io::stderr().lock();
            let _ = write!(stderr, "\r\x1b[K");
            let _ = stderr.flush();
        }
    }

    fn stop_thread(&self) {
        self.stop.store(true, Ordering::Release);
        let handle = lock(&self.thread).take();
        if let Some(handle) = handle {
            handle.thread().unpark();
            let _ = handle.join();
        }
        lock(&self.message).clear();
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let handle = lock(&self.thread).take();
        if let Some(handle) = handle {
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
