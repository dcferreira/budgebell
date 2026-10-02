//! Thin wrapper for the Linux probes that shell out to a desktop tool
//! (`gdbus`, `gsettings`, `pactl`). Those tools are optional — a minimal
//! install, a non-GNOME desktop, or a missing sound server all legitimately
//! lack them — so "the tool is unavailable" is reported as `None` and each
//! probe degrades to "clear". Unparsable output from a tool that *did* answer
//! is a different matter and stays a loud error in the probe itself.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long a probe tool may run before it is killed and treated as
/// unavailable. The probes run while the app lock is held, so a hung
/// gnome-shell or pipewire-pulse must not be allowed to freeze the app.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// How often a running tool is polled for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Runs `program` with `args` and returns its stdout, or `None` when the tool
/// is unavailable: it could not be spawned (not installed), exited nonzero
/// (no such schema, no session bus, unsupported method, no sound server), or
/// did not finish within `PROBE_TIMEOUT` (it is killed and reaped).
///
/// The tool runs with `LC_ALL=C` so its output is in the stable,
/// untranslated machine-readable form the parsers expect, whatever the
/// user's locale. Stderr is discarded.
///
/// Degrading rather than erroring is deliberate: an unconditional error would
/// fail every scheduler tick on a machine that simply lacks the tool, and
/// silently disable reminders altogether.
pub fn run_stdout_if_available(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Drain stdout on a helper thread while we poll for exit. Waiting for exit
    // first and reading afterwards could deadlock on output bigger than the
    // pipe buffer (the child blocks writing, we block waiting); polling
    // `try_wait` while also reading avoids that and still lets us enforce the
    // deadline, which a blocking `read_to_end` on this thread could not.
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            // Timed out or the wait itself failed: kill and reap so no zombie
            // or runaway process is left behind, then degrade. The reader
            // thread ends by itself when the pipe closes.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    // The child has exited, so stdout reaches EOF — unless a grandchild
    // inherited the pipe, hence the bounded wait rather than a blocking one.
    let remaining = deadline.saturating_duration_since(Instant::now());
    let bytes = receiver.recv_timeout(remaining).ok()?.ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_program_that_is_not_installed_is_unavailable() {
        // Given a program that does not exist on PATH
        // When run
        let output = run_stdout_if_available("budgebell-no-such-tool", &[]);

        // Then it is unavailable rather than an error
        assert_eq!(output, None);
    }

    #[test]
    fn a_program_exiting_nonzero_is_unavailable() {
        // Given a program that exits with a failure status
        // When run
        let output = run_stdout_if_available("false", &[]);

        // Then it is unavailable
        assert_eq!(output, None);
    }

    #[test]
    fn a_program_that_hangs_is_unavailable_once_the_timeout_elapses() {
        // Given a program that would run far longer than the probe timeout
        let started = Instant::now();

        // When run
        let output = run_stdout_if_available("sleep", &["5"]);

        // Then it is killed and reported unavailable well before it finishes
        assert_eq!(output, None);
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_program_that_succeeds_through_a_shell_yields_its_stdout() {
        // Given a shell command that prints a line
        // When run
        let output = run_stdout_if_available("sh", &["-c", "echo hi"]);

        // Then its stdout is returned
        assert_eq!(output.as_deref(), Some("hi\n"));
    }

    #[test]
    fn output_larger_than_a_pipe_buffer_does_not_deadlock() {
        // Given a program printing ~1 MB, far beyond the 64 KiB pipe buffer
        // When run
        let output = run_stdout_if_available("sh", &["-c", "yes | head -c 1000000"]);

        // Then all of it is returned instead of the child blocking on a full pipe
        assert_eq!(output.map(|text| text.len()), Some(1_000_000));
    }

    #[test]
    fn the_command_runs_with_the_c_locale() {
        // Given a program that echoes LC_ALL from its environment
        // When run
        let output = run_stdout_if_available("sh", &["-c", "printf %s \"$LC_ALL\""]);

        // Then it sees the stable C locale
        assert_eq!(output.as_deref(), Some("C"));
    }

    #[test]
    fn a_program_exiting_zero_yields_its_stdout() {
        // Given a program that succeeds and prints a line
        // When run
        let output = run_stdout_if_available("echo", &["hello"]);

        // Then its stdout is returned verbatim
        assert_eq!(output.as_deref(), Some("hello\n"));
    }
}
