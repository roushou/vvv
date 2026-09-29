//! Windows containment and nonblocking byte pipes, through safe dependency APIs.
use super::*;
use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};
use process_wrap::std::{ChildWrapper, CommandWrap, JobObject};
use std::{
    io,
    os::windows::io::OwnedHandle,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::atomic::AtomicU64,
};

pub(super) struct Reader(std::fs::File);
impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // FileExt preserves ERROR_NO_DATA, unlike Read implementations that turn
        // every BrokenPipe error into EOF. Pipe byte offsets are ignored.
        use std::os::windows::fs::FileExt;
        match self.0.seek_read(buffer, 0) {
            Err(error) if error.raw_os_error() == Some(232) => {
                Err(io::ErrorKind::WouldBlock.into())
            }
            Err(error) if matches!(error.raw_os_error(), Some(109 | 233)) => Ok(0),
            result => result,
        }
    }
}

pub(crate) struct CheckProcess {
    child: Box<dyn ChildWrapper>,
}
impl CheckProcess {
    pub(super) fn spawn(
        command: &CheckCommand,
        root: &Path,
    ) -> Result<(Self, Reader, Reader), CheckFailure> {
        let program = Self::program(&command.program, root)
            .map_err(|e| CheckFailure::io(CheckOperation::Spawn, e))?;
        let (out, out_writer) =
            Self::pipe().map_err(|e| CheckFailure::io(CheckOperation::Capture, e))?;
        let (err, err_writer) =
            Self::pipe().map_err(|e| CheckFailure::io(CheckOperation::Capture, e))?;
        let mut native = Command::new(program);
        native.args(&command.args);
        native
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out_writer))
            .stderr(Stdio::from(err_writer));
        let mut wrapped = CommandWrap::from(native);
        wrapped.wrap(JobObject);
        let child = wrapped
            .spawn()
            .map_err(|e| CheckFailure::io(CheckOperation::Spawn, e))?;
        // Drop the Command's copies of pipe writers before capture waits for EOF.
        drop(wrapped);
        Ok((Self { child }, out, err))
    }

    fn pipe() -> io::Result<(Reader, std::fs::File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            r"\\.\pipe\vvv-check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let listener = PipeListenerOptions::new()
            .path(name.as_str())
            .nonblocking(true)
            .create_recv_only::<pipe_mode::Bytes>()?;
        // Synchronous write handle for the child's standard streams.
        let writer = std::fs::OpenOptions::new().write(true).open(&name)?;
        let reader = listener.accept()?;
        reader.set_nonblocking(true)?;
        let handle =
            OwnedHandle::try_from(reader).map_err(|_| io::Error::other("split pipe reader"))?;
        let reader = Reader(std::fs::File::from(handle));
        Ok((reader, writer))
    }

    fn program(program: &str, root: &Path) -> io::Result<PathBuf> {
        let mut path = PathBuf::from(program);
        match path.extension() {
            Some(extension) if extension.eq_ignore_ascii_case("exe") => {}
            None => {
                path.set_extension("exe");
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "native executable required",
                ));
            }
        }
        if path.is_absolute() || path.components().count() > 1 {
            return Ok(root.join(path));
        }
        // Resolve only .exe files, never a batch file that std would launch via cmd.exe.
        std::iter::once(root.to_path_buf())
            .chain(
                std::env::var_os("PATH")
                    .into_iter()
                    .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>()),
            )
            .map(|directory| root.join(directory).join(&path))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| io::Error::from_raw_os_error(2))
    }

    pub(super) fn poll(&mut self) -> io::Result<Option<()>> {
        // Poll the leader, not job completion: descendants are killed when the leader exits.
        self.child
            .inner_mut()
            .try_wait()
            .map(|status| status.map(|_| ()))
    }

    pub(super) fn terminate(&mut self) -> Result<ExitStatus, CheckFailure> {
        if let Err(error) = self.child.start_kill() {
            let _ = self.child.inner_mut().start_kill();
            return Err(CheckFailure::io(CheckOperation::Terminate, error));
        }
        let status = self
            .child
            .inner_mut()
            .wait()
            .map_err(|e| CheckFailure::io(CheckOperation::Wait, e))?;
        Ok(status)
    }
}
impl Drop for CheckProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn empty_pipe_is_stoppable_and_only_closed_writer_means_eof() {
        let (mut reader, mut writer) = CheckProcess::pipe().unwrap();
        let mut buffer = [0; 32];
        let started = Instant::now();
        assert_eq!(
            reader.read(&mut buffer).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        writer.write_all("héllo 世界".as_bytes()).unwrap();
        let count = reader.read(&mut buffer).unwrap();
        assert_eq!(&buffer[..count], "héllo 世界".as_bytes());
        assert_eq!(
            reader.read(&mut buffer).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(writer);
        assert_eq!(reader.read(&mut buffer).unwrap(), 0);
    }

    #[test]
    fn native_program_resolution_rejects_implicit_shells() {
        let root = std::env::temp_dir();
        for program in ["check.cmd", "check.BAT", "check.ps1"] {
            assert_eq!(
                CheckProcess::program(program, &root).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(
            CheckProcess::program("tools/检查 程序", &root).unwrap(),
            root.join("tools/检查 程序.exe")
        );
        assert_eq!(
            CheckProcess::program("tools/check.EXE", &root).unwrap(),
            root.join("tools/check.EXE")
        );
    }
}
