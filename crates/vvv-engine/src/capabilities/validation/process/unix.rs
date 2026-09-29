use super::*;
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid},
};
use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};
pub(crate) struct CheckProcess {
    child: std::process::Child,
}
impl CheckProcess {
    pub(super) fn spawn(
        command: &CheckCommand,
        root: &Path,
    ) -> Result<(Self, std::process::ChildStdout, std::process::ChildStderr), CheckFailure> {
        let child = Command::new(&command.program)
            .args(&command.args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|e| CheckFailure::io(CheckOperation::Spawn, e))?;
        let mut process = Self { child };
        let out = process.child.stdout.take().expect("piped stdout");
        let err = process.child.stderr.take().expect("piped stderr");
        let setup = || -> std::io::Result<()> {
            fcntl_setfl(&out, fcntl_getfl(&out)? | OFlags::NONBLOCK)?;
            fcntl_setfl(&err, fcntl_getfl(&err)? | OFlags::NONBLOCK)?;
            Ok(())
        };
        if let Err(error) = setup() {
            process.terminate()?;
            return Err(CheckFailure::io(CheckOperation::Capture, error));
        }
        Ok((process, out, err))
    }
    pub(super) fn poll(&mut self) -> std::io::Result<Option<()>> {
        // Keep the leader unreaped until cleanup so its group id cannot be recycled.
        let pid = Pid::from_raw(self.child.id() as i32).expect("child pid");
        waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|status| status.map(|_| ()))
        .map_err(Into::into)
    }
    pub(super) fn terminate(&mut self) -> Result<std::process::ExitStatus, CheckFailure> {
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
