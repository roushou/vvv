//! Bounded, stoppable subprocess capture. No shell interpretation or stdin inheritance.
use super::{CheckCommand, CheckFailure, CheckOperation, CheckOutcome, CheckResult};
use crate::ReadCancellation;
use std::{path::Path, time::Instant};

pub(super) struct CheckProcess {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd"
    ))]
    child: std::process::Child,
}
impl CheckProcess {
    pub(super) fn supported() -> bool {
        cfg!(any(
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
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
mod unix {
    use super::super::CheckOutput;
    use super::*;
    use rustix::{
        fd::AsFd,
        fs::{OFlags, fcntl_getfl, fcntl_setfl},
        process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid},
    };
    use std::{
        io::Read,
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };
    struct Capture {
        stop: Arc<AtomicBool>,
        done: Arc<AtomicBool>,
        worker: thread::JoinHandle<(CheckOutput, Option<CheckFailure>)>,
    }
    impl Capture {
        fn start(
            mut reader: impl Read + AsFd + Send + 'static,
            limit: usize,
        ) -> Result<Self, CheckFailure> {
            let flags = fcntl_getfl(&reader)
                .map_err(|error| CheckFailure::io(CheckOperation::Capture, error.into()))?;
            fcntl_setfl(&reader, flags | OFlags::NONBLOCK)
                .map_err(|error| CheckFailure::io(CheckOperation::Capture, error.into()))?;
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
            let child = Command::new(&command.program)
                .args(&command.args)
                .current_dir(root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn();
            let mut process = match child {
                Ok(child) => Self { child },
                Err(error) => {
                    result.outcome = CheckOutcome::Error;
                    result.failure = Some(CheckFailure::io(CheckOperation::Spawn, error));
                    return result;
                }
            };
            let stdout = Capture::start(process.child.stdout.take().expect("piped stdout"), limit);
            let stderr = Capture::start(process.child.stderr.take().expect("piped stderr"), limit);
            if stdout.is_err() || stderr.is_err() {
                let _ = process.terminate();
                result.outcome = CheckOutcome::Error;
                for capture in [stdout, stderr] {
                    match capture {
                        Ok(capture) => {
                            capture.finish();
                        }
                        Err(error) => result.failure = Some(error),
                    }
                }
                return result;
            }
            let stdout = stdout.unwrap_or_else(|_| unreachable!());
            let stderr = stderr.unwrap_or_else(|_| unreachable!());
            let pid = Pid::from_raw(process.child.id() as i32).expect("child pid");
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
                // Leave the exited leader unreaped until group cleanup. Its pid
                // cannot be recycled into an unrelated process group meanwhile.
                match waitid(
                    WaitId::Pid(pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Ok(Some(_)) => {
                        result.outcome = CheckOutcome::Passed;
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(10)),
                    Err(error) if error == rustix::io::Errno::INTR => {}
                    Err(error) => {
                        result.outcome = CheckOutcome::Error;
                        result.failure = Some(CheckFailure::io(CheckOperation::Wait, error.into()));
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
        fn terminate(&mut self) -> Result<std::process::ExitStatus, CheckFailure> {
            let pid = Pid::from_raw(self.child.id() as i32).expect("child pid");
            let group_error = kill_process_group(pid, Signal::KILL)
                .err()
                .filter(|error| *error != rustix::io::Errno::SRCH);
            // Also kill the direct child if it escaped its original group.
            if let Err(error) = self.child.kill() {
                if let Some(status) = self
                    .child
                    .try_wait()
                    .map_err(|error| CheckFailure::io(CheckOperation::Wait, error))?
                {
                    return group_error.map_or(Ok(status), |error| {
                        Err(CheckFailure::io(CheckOperation::Terminate, error.into()))
                    });
                }
                return Err(CheckFailure::io(CheckOperation::Terminate, error));
            }
            let status = self
                .child
                .wait()
                .map_err(|error| CheckFailure::io(CheckOperation::Wait, error))?;
            group_error.map_or(Ok(status), |error| {
                Err(CheckFailure::io(CheckOperation::Terminate, error.into()))
            })
        }
    }
}
