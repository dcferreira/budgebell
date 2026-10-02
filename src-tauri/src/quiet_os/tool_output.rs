//! Thin wrapper for the Linux probes that shell out to a desktop tool
//! (`gdbus`, `gsettings`, `pactl`). Those tools are optional — a minimal
//! install, a non-GNOME desktop, or a missing sound server all legitimately
//! lack them — so "the tool is unavailable" is reported as `None` and each
//! probe degrades to "clear". Unparsable output from a tool that *did* answer
//! is a different matter and stays a loud error in the probe itself.

use std::process::Command;

/// Runs `program` with `args` and returns its stdout, or `None` when the tool
/// is unavailable: it could not be spawned (not installed) or exited nonzero
/// (no such schema, no session bus, unsupported method, no sound server).
///
/// Degrading rather than erroring is deliberate: an unconditional error would
/// fail every scheduler tick on a machine that simply lacks the tool, and
/// silently disable reminders altogether.
pub fn run_stdout_if_available(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_program_exiting_zero_yields_its_stdout() {
        // Given a program that succeeds and prints a line
        // When run
        let output = run_stdout_if_available("echo", &["hello"]);

        // Then its stdout is returned verbatim
        assert_eq!(output.as_deref(), Some("hello\n"));
    }
}
