//! Bounded CLI transport shared by Docker and the read-only app inventory.
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub fn run(command: &str, args: &[&str], timeout: Duration, label: &str) -> Result<String, String> {
    run_config(command, args, timeout, label, &[], &[0], None)
}

pub fn run_config(
    command: &str,
    args: &[&str],
    timeout: Duration,
    label: &str,
    env: &[(&str, &str)],
    accepted: &[i32],
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<String, String> {
    let mut child = Command::new(command)
        .args(args)
        .envs(env.iter().copied())
        .process_group(0)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                crate::tf!(
                    "{0} is not installed or unavailable in the launch path.",
                    label
                )
            } else {
                e.to_string()
            }
        })?;
    // Drain both pipes while the child runs; a large listing cannot fill the pipe and hang.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(2_000_001)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(65_537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None)
                if start.elapsed() < timeout
                    && !cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            result => {
                if matches!(result, Ok(None)) {
                    // The unreaped CLI is the leader of its own new process group.
                    // Reap its helpers too (e.g. SSH for a remote Docker context),
                    // so inherited pipes cannot keep the reader threads blocked.
                    // SAFETY: this group belongs exclusively to the child we created.
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(match result {
                    Err(error) => error.to_string(),
                    _ if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) => {
                        format!("{label} abgebrochen.")
                    }
                    _ => crate::tf!("{0} call timed out.", label),
                });
            }
        }
    };
    let output = output
        .join()
        .map_err(|_| crate::tr!("Unable to read program output."))?
        .map_err(|e| e.to_string())?;
    let errors = errors
        .join()
        .map_err(|_| crate::tr!("Unable to read program errors."))?
        .map_err(|e| e.to_string())?;
    let status = status?;
    if output.len() > 2_000_000 || errors.len() > 65_536 {
        return Err(crate::tf!("{0} output exceeds the supported size.", label));
    }
    if !status.code().is_some_and(|code| accepted.contains(&code)) {
        let error = String::from_utf8_lossy(&errors);
        return Err(if error.contains("permission denied") {
            crate::tf!(
                "Cannot access {0}. Check the permissions of your normal session. The app does not change permissions.",
                label
            )
        } else {
            crate::tf!(
                "{0} call failed: {1}",
                label,
                error.trim().chars().take(400).collect::<String>()
            )
        });
    }
    String::from_utf8(output)
        .map_err(|_| crate::tf!("{0} did not return valid text output.", label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[test]
    fn accepted_no_update_exit_errors_and_environment_are_distinct() {
        assert_eq!(
            run_config(
                "/bin/sh",
                &["-c", "printf '%s' \"$TASK_MANAGER_TEST_VALUE\"; exit 2"],
                Duration::from_secs(2),
                "Test",
                &[("TASK_MANAGER_TEST_VALUE", "own value")],
                &[0, 2],
                None
            )
            .unwrap(),
            "own value"
        );
        assert!(
            run(
                "/bin/sh",
                &["-c", "printf 'failed' >&2; exit 1"],
                Duration::from_secs(2),
                "Test"
            )
            .unwrap_err()
            .contains("failed")
        );
    }
    #[test]
    fn cancellation_only_stops_the_disposable_command_group() {
        let token = Arc::new(AtomicBool::new(false));
        let other = token.clone();
        let thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            other.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let error = run_config(
            "/bin/sh",
            &["-c", "sleep 5"],
            Duration::from_secs(5),
            "Test",
            &[],
            &[0],
            Some(&token),
        )
        .unwrap_err();
        thread.join().unwrap();
        assert!(error.contains("abgebrochen"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
