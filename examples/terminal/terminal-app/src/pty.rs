//! A program in a pseudo-terminal: the only IO a session does.

use std::fs::File;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use rustix::fs::{Mode, OFlags};
use rustix::process::{Pid, Signal};
use rustix::pty::OpenptFlags;
use rustix::termios::Winsize;
use terminal_core::Size;

/// A running program and the controlling side of its terminal. Dropping it
/// hangs up the program.
pub struct Pty {
    master: OwnedFd,
    /// Taken by `drop`, which hands it to a thread that reaps it.
    child: Option<Child>,
}

fn winsize(size: Size) -> Winsize {
    Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

impl Pty {
    /// Starts `argv` in a new terminal of `size`, as the leader of its own
    /// session with the terminal as its controlling terminal.
    pub fn spawn(argv: &[String], size: Size, terminfo: Option<&Path>) -> io::Result<Self> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no program to run"))?;
        let master = rustix::pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
        rustix::pty::grantpt(&master)?;
        rustix::pty::unlockpt(&master)?;
        rustix::termios::tcsetwinsize(&master, winsize(size))?;
        let name = rustix::pty::ptsname(&master, Vec::new())?;
        let slave = rustix::fs::open(
            name.as_c_str(),
            OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave))
            .env(
                "TERM",
                if terminfo.is_some() {
                    "xterm-ghostty"
                } else {
                    "xterm-256color"
                },
            )
            .env("COLORTERM", "truecolor");
        if let Some(dir) = terminfo {
            command.env("TERMINFO", dir);
        }
        // SAFETY: the closure runs in the child between fork and exec, and
        // only makes two system calls.
        unsafe {
            command.pre_exec(|| {
                rustix::process::setsid()?;
                // Standard input is the terminal; make it the controlling one,
                // so programs that open /dev/tty (fzf, ssh) get this terminal.
                rustix::process::ioctl_tiocsctty(BorrowedFd::borrow_raw(0))?;
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(Self {
            master,
            child: Some(child),
        })
    }

    /// A handle to read the program's output.
    pub fn reader(&self) -> io::Result<File> {
        Ok(File::from(self.master.try_clone()?))
    }

    /// A handle to write the program's input.
    pub fn writer(&self) -> io::Result<File> {
        Ok(File::from(self.master.try_clone()?))
    }

    pub fn resize(&self, size: Size) -> io::Result<()> {
        Ok(rustix::termios::tcsetwinsize(
            self.master.as_fd(),
            winsize(size),
        )?)
    }

    /// The exit code, once the program has ended.
    pub fn exit_code(&mut self) -> Option<Option<i32>> {
        let child = self.child.as_mut()?;
        child.try_wait().ok().flatten().map(|status| status.code())
    }
}

/// How long a hung-up program gets to exit before it is killed.
const HANGUP_GRACE: Duration = Duration::from_secs(1);

impl Drop for Pty {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        // Hang up the whole session, as closing a terminal window does, so
        // programs can save their work. Kill what is left after a grace
        // period, then reap it, on a thread so the caller does not wait.
        let _ = rustix::process::kill_process_group(Pid::from_child(&child), Signal::HUP);
        let _ = thread::Builder::new()
            .name("pty-reaper".into())
            .spawn(move || {
                let step = Duration::from_millis(20);
                let mut waited = Duration::ZERO;
                while waited < HANGUP_GRACE {
                    if child.try_wait().ok().flatten().is_some() {
                        return;
                    }
                    thread::sleep(step);
                    waited += step;
                }
                let _ = child.kill();
                let _ = child.wait();
            });
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    fn read_until(reader: &mut File, needle: &str) -> String {
        let mut seen = String::new();
        let mut buf = [0; 1024];
        while !seen.contains(needle) {
            let n = reader.read(&mut buf).unwrap();
            assert!(
                n > 0,
                "the program ended before printing {needle:?}: {seen:?}"
            );
            seen.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        seen
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), script.into()]
    }

    #[test]
    fn the_program_sees_a_terminal_of_the_given_size() {
        let size = Size { cols: 77, rows: 11 };
        let pty = Pty::spawn(&sh("stty size; tty -s && echo tty"), size, None).unwrap();

        let output = read_until(&mut pty.reader().unwrap(), "tty");

        assert!(output.contains("11 77"), "{output:?}");
    }

    #[test]
    fn input_reaches_the_program_and_resizes_reach_it_too() {
        let pty = Pty::spawn(
            &sh("read line; echo got:$line; stty size"),
            Size::default(),
            None,
        )
        .unwrap();
        pty.resize(Size { cols: 40, rows: 5 }).unwrap();
        pty.writer().unwrap().write_all(b"hello\r").unwrap();

        let output = read_until(&mut pty.reader().unwrap(), "5 40");

        assert!(output.contains("got:hello"), "{output:?}");
    }

    #[test]
    fn the_terminal_is_the_controlling_one() {
        let pty = Pty::spawn(&sh("echo hi > /dev/tty; echo done"), Size::default(), None).unwrap();

        let output = read_until(&mut pty.reader().unwrap(), "done");

        assert!(output.contains("hi"), "{output:?}");
    }

    #[test]
    fn the_exit_code_is_known_once_the_program_ends() {
        let mut pty = Pty::spawn(&sh("exit 3"), Size::default(), None).unwrap();
        let mut reader = pty.reader().unwrap();
        // The terminal reports an error or end of file once the program is gone.
        let _ = reader.read_to_end(&mut Vec::new());

        let code = (0..100).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            pty.exit_code()
        });

        assert_eq!(code, Some(Some(3)));
    }

    #[test]
    fn dropping_it_ends_the_program() {
        let pty = Pty::spawn(&sh("echo ready; sleep 30"), Size::default(), None).unwrap();
        let mut reader = pty.reader().unwrap();
        read_until(&mut reader, "ready");
        let started = std::time::Instant::now();

        drop(pty);
        // Output ends once the program is gone, not after its 30 seconds.
        let _ = reader.read_to_end(&mut Vec::new());

        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
