//! Bounded, stoppable subprocess capture. No stdin inheritance.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
use super::CheckOutput;
use super::{CheckCommand, CheckFailure, CheckOperation, CheckOutcome, CheckResult};
use crate::ReadCancellation;
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use std::{path::Path, time::Instant};
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
mod unix;
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
pub(super) use unix::CheckProcess;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::CheckProcess;
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
)))]
pub(super) struct CheckProcess;
impl CheckProcess {
    pub(super) fn supported() -> bool {
        cfg!(any(
            windows,
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd"
        ))
    }
    pub(super) fn run(
        command: &CheckCommand,
        root: &Path,
        deadline: Instant,
        limit: usize,
        cancellation: Option<ReadCancellation>,
    ) -> CheckResult {
        #[cfg(any(
            windows,
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd"
        ))]
        {
            Self::execute(command, root, deadline, limit, cancellation)
        }
        #[cfg(not(any(
            windows,
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd"
        )))]
        {
            let _ = (root, deadline, limit, cancellation);
            let mut result = CheckResult::pending(command.clone());
            result.outcome = CheckOutcome::Error;
            result.failure = Some(CheckFailure {
                operation: CheckOperation::Spawn,
                os_code: None,
            });
            result
        }
    }
}
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
struct Capture {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    worker: thread::JoinHandle<(CheckOutput, Option<CheckFailure>)>,
}
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
impl Capture {
    fn start(mut reader: impl Read + Send + 'static, limit: usize) -> Result<Self, CheckFailure> {
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let finished = done.clone();
        let worker = thread::Builder::new()
            .name("vvv-check-output".into())
            .spawn(move || {
                let mut output = CheckOutput::default();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 8192];
                let mut failure = None;
                while !cancel.load(Ordering::Acquire) {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            output.complete = true;
                            break;
                        }
                        Ok(count) => {
                            output.bytes_seen = output.bytes_seen.saturating_add(count as u64);
                            let remaining = limit.saturating_sub(bytes.len());
                            bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                            output.truncated |= count > remaining;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            failure = Some(CheckFailure::io(CheckOperation::Capture, error));
                            break;
                        }
                    }
                }
                output.text = String::from_utf8_lossy(&bytes).into_owned();
                finished.store(true, Ordering::Release);
                (output, failure)
            })
            .map_err(|error| CheckFailure::io(CheckOperation::Capture, error))?;
        Ok(Self { stop, done, worker })
    }
    fn finish(self) -> (CheckOutput, Option<CheckFailure>) {
        self.stop.store(true, Ordering::Release);
        self.worker.join().unwrap_or_else(|_| {
            (
                CheckOutput::default(),
                Some(CheckFailure {
                    operation: CheckOperation::Capture,
                    os_code: None,
                }),
            )
        })
    }
}
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
impl CheckProcess {
    pub(super) fn execute(
        command: &CheckCommand,
        root: &Path,
        deadline: Instant,
        limit: usize,
        cancellation: Option<ReadCancellation>,
    ) -> CheckResult {
        let start = Instant::now();
        let mut result = CheckResult::pending(command.clone());
        if cancellation
            .as_ref()
            .is_some_and(ReadCancellation::is_cancelled)
        {
            result.outcome = CheckOutcome::Cancelled;
            return result;
        }
        if Instant::now() >= deadline {
            result.outcome = CheckOutcome::TimedOut;
            return result;
        }
        let (mut process, out, err) = match Self::spawn(command, root) {
            Ok(process) => process,
            Err(failure) => {
                result.outcome = CheckOutcome::Error;
                result.failure = Some(failure);
                return result;
            }
        };
        let stdout = Capture::start(out, limit);
        let stderr = Capture::start(err, limit);
        if stdout.is_err() || stderr.is_err() {
            let cleanup = process.terminate();
            result.outcome = CheckOutcome::Error;
            for (capture, output) in [(stdout, &mut result.stdout), (stderr, &mut result.stderr)] {
                match capture {
                    Ok(capture) => {
                        let (captured, failure) = capture.finish();
                        *output = captured;
                        result.failure = result.failure.or(failure);
                    }
                    Err(error) => result.failure = Some(error),
                }
            }
            match cleanup {
                Ok(status) => result.exit_code = status.code(),
                Err(error) => result.failure = Some(error),
            }
            result.duration_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            return result;
        }
        let stdout = stdout.unwrap_or_else(|_| unreachable!());
        let stderr = stderr.unwrap_or_else(|_| unreachable!());
        loop {
            if cancellation
                .as_ref()
                .is_some_and(ReadCancellation::is_cancelled)
            {
                result.outcome = CheckOutcome::Cancelled;
                break;
            }
            if Instant::now() >= deadline {
                result.outcome = CheckOutcome::TimedOut;
                break;
            }
            match process.poll() {
                Ok(Some(_)) => {
                    result.outcome = CheckOutcome::Passed;
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    result.outcome = CheckOutcome::Error;
                    result.failure = Some(CheckFailure::io(CheckOperation::Wait, error));
                    break;
                }
            }
        }
        match process.terminate() {
            Ok(status) => {
                result.exit_code = status.code();
                if result.outcome == CheckOutcome::Passed && !status.success() {
                    result.outcome = CheckOutcome::Failed;
                }
            }
            Err(error) => {
                result.outcome = CheckOutcome::Error;
                result.failure = Some(error);
            }
        }
        let drain = Instant::now() + Duration::from_millis(100);
        while (!stdout.done.load(Ordering::Acquire) || !stderr.done.load(Ordering::Acquire))
            && Instant::now() < drain
        {
            thread::sleep(Duration::from_millis(5));
        }
        let (out, out_failure) = stdout.finish();
        let (err, err_failure) = stderr.finish();
        result.stdout = out;
        result.stderr = err;
        if let Some(failure) = out_failure.or(err_failure) {
            result.outcome = CheckOutcome::Error;
            result.failure = Some(failure);
        }
        if result.outcome == CheckOutcome::Passed
            && (!result.stdout.complete || !result.stderr.complete)
        {
            result.outcome = CheckOutcome::Error;
            result.failure = Some(CheckFailure {
                operation: CheckOperation::Capture,
                os_code: None,
            });
        }
        result.duration_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        result
    }
}
