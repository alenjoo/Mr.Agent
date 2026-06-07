use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::types::{
    TerminalCommandRequest, TerminalCommandResult, TerminalCommandStatus, TerminalSafetyDecision,
};

const DEFAULT_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
const DEFAULT_ALLOWED_ROOT: &str = "/Users/alenjosephjohn/Multi";

pub fn run_terminal_command(request: TerminalCommandRequest) -> TerminalCommandResult {
    let start = Instant::now();
    let command = request.command.trim().to_string();
    let requested_cwd = request.workdir.as_deref();
    let allowed_root = request
        .allowed_root
        .as_deref()
        .unwrap_or(DEFAULT_ALLOWED_ROOT);

    let cwd_result = resolve_workdir(requested_cwd, allowed_root);
    let cwd = cwd_result
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|error| error.clone());

    if command.is_empty() {
        return blocked_result(command, cwd, start, "command is empty");
    }

    let safety = classify_command_safety(&command);
    if !safety.allowed {
        return blocked_result(
            command,
            cwd,
            start,
            safety.reason.as_deref().unwrap_or("command blocked"),
        );
    }

    let cwd = match cwd_result {
        Ok(path) => path,
        Err(reason) => {
            return blocked_result(command, reason, start, "workdir outside allowed root")
        }
    };

    let timeout = Duration::from_secs(request.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS));
    let max_output_bytes = request
        .max_output_bytes
        .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
        .max(1);

    execute_local(command, cwd, timeout, max_output_bytes, start)
}

pub fn classify_command_safety(command: &str) -> TerminalSafetyDecision {
    let normalized = command.to_lowercase();
    let blocked_patterns = [
        (
            "rm -rf /",
            "command matched destructive root deletion pattern",
        ),
        ("sudo", "sudo is not allowed in this terminal tool version"),
        ("mkfs", "filesystem formatting commands are blocked"),
        ("dd if=", "raw disk copy commands are blocked"),
        (
            "chmod -r 777 /",
            "recursive broad permission changes are blocked",
        ),
        ("chown -r", "recursive ownership changes are blocked"),
        (":(){", "fork bomb pattern is blocked"),
        (".ssh", "direct access to ssh secrets is blocked"),
        ("id_rsa", "direct access to private keys is blocked"),
        ("id_ed25519", "direct access to private keys is blocked"),
        (".env", "direct access to env files is blocked"),
        (
            "curl",
            "network exfiltration-like commands are blocked for now",
        ),
        (
            "wget",
            "network exfiltration-like commands are blocked for now",
        ),
    ];

    for (pattern, reason) in blocked_patterns {
        if normalized.contains(pattern) {
            return TerminalSafetyDecision {
                allowed: false,
                reason: Some(reason.to_string()),
            };
        }
    }

    TerminalSafetyDecision {
        allowed: true,
        reason: None,
    }
}

fn execute_local(
    command: String,
    cwd: PathBuf,
    timeout: Duration,
    max_output_bytes: usize,
    start: Instant,
) -> TerminalCommandResult {
    let mut child = match Command::new("bash")
        .arg("-lc")
        .arg(&command)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return TerminalCommandResult {
                command,
                cwd: cwd.to_string_lossy().to_string(),
                status: TerminalCommandStatus::Failed,
                exit_code: None,
                stdout: String::new(),
                stderr: error.to_string(),
                duration_ms: start.elapsed().as_millis(),
                truncated: false,
                safety: TerminalSafetyDecision {
                    allowed: true,
                    reason: None,
                },
            }
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = thread::spawn(move || read_pipe(stdout));
    let stderr_handle = thread::spawn(move || read_pipe(stderr));

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();
    let (stdout, stdout_truncated) = truncate_bytes(stdout_bytes, max_output_bytes);
    let (stderr, stderr_truncated) = truncate_bytes(stderr_bytes, max_output_bytes);

    let (status_kind, exit_code) = match status {
        Some(exit_status) if exit_status.success() => {
            (TerminalCommandStatus::Success, exit_status.code())
        }
        Some(exit_status) => (TerminalCommandStatus::Failed, exit_status.code()),
        None => (TerminalCommandStatus::TimedOut, None),
    };

    TerminalCommandResult {
        command,
        cwd: cwd.to_string_lossy().to_string(),
        status: status_kind,
        exit_code,
        stdout,
        stderr,
        duration_ms: start.elapsed().as_millis(),
        truncated: stdout_truncated || stderr_truncated,
        safety: TerminalSafetyDecision {
            allowed: true,
            reason: None,
        },
    }
}

fn read_pipe(pipe: Option<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    bytes
}

fn truncate_bytes(bytes: Vec<u8>, max_output_bytes: usize) -> (String, bool) {
    let truncated = bytes.len() > max_output_bytes;
    let mut limited = if truncated {
        bytes[..max_output_bytes].to_vec()
    } else {
        bytes
    };
    if truncated {
        limited.extend_from_slice(b"\n[output truncated]");
    }
    (String::from_utf8_lossy(&limited).to_string(), truncated)
}

fn resolve_workdir(workdir: Option<&str>, allowed_root: &str) -> Result<PathBuf, String> {
    let cwd = match workdir {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => std::env::current_dir().map_err(|error| error.to_string())?,
    };
    let canonical_cwd = canonicalize_existing(&cwd)?;
    let canonical_root = canonicalize_existing(Path::new(allowed_root))?;

    if !canonical_cwd.starts_with(&canonical_root) {
        return Err(format!(
            "{} is outside allowed root {}",
            canonical_cwd.to_string_lossy(),
            canonical_root.to_string_lossy()
        ));
    }

    Ok(canonical_cwd)
}

fn canonicalize_existing(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    path.as_ref()
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.as_ref().to_string_lossy()))
}

fn blocked_result(
    command: String,
    cwd: String,
    start: Instant,
    reason: impl Into<String>,
) -> TerminalCommandResult {
    let reason = reason.into();
    TerminalCommandResult {
        command,
        cwd,
        status: TerminalCommandStatus::Blocked,
        exit_code: None,
        stdout: String::new(),
        stderr: reason.clone(),
        duration_ms: start.elapsed().as_millis(),
        truncated: false,
        safety: TerminalSafetyDecision {
            allowed: false,
            reason: Some(reason),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(command: &str) -> TerminalCommandRequest {
        TerminalCommandRequest {
            command: command.to_string(),
            workdir: Some("/Users/alenjosephjohn/Multi/ownpager".to_string()),
            timeout_seconds: Some(5),
            max_output_bytes: Some(1024),
            allowed_root: Some("/Users/alenjosephjohn/Multi".to_string()),
        }
    }

    #[test]
    fn rejects_empty_command() {
        let result = run_terminal_command(request("   "));
        assert_eq!(result.status, TerminalCommandStatus::Blocked);
    }

    #[test]
    fn executes_valid_command() {
        let result = run_terminal_command(request("printf hello"));
        assert_eq!(result.status, TerminalCommandStatus::Success);
        assert_eq!(result.stdout, "hello");
    }

    #[test]
    fn captures_non_zero_exit_code_as_failed_result() {
        let result = run_terminal_command(request("exit 7"));
        assert_eq!(result.status, TerminalCommandStatus::Failed);
        assert_eq!(result.exit_code, Some(7));
    }

    #[test]
    fn captures_stderr() {
        let result = run_terminal_command(request("printf problem >&2"));
        assert_eq!(result.status, TerminalCommandStatus::Success);
        assert_eq!(result.stderr, "problem");
    }

    #[test]
    fn times_out_long_command() {
        let mut req = request("sleep 2");
        req.timeout_seconds = Some(1);
        let result = run_terminal_command(req);
        assert_eq!(result.status, TerminalCommandStatus::TimedOut);
    }

    #[test]
    fn truncates_large_output() {
        let mut req = request("printf abcdef");
        req.max_output_bytes = Some(3);
        let result = run_terminal_command(req);
        assert_eq!(result.status, TerminalCommandStatus::Success);
        assert!(result.truncated);
        assert!(result.stdout.starts_with("abc"));
    }

    #[test]
    fn rejects_workdir_outside_allowed_root() {
        let mut req = request("pwd");
        req.workdir = Some("/tmp".to_string());
        let result = run_terminal_command(req);
        assert_eq!(result.status, TerminalCommandStatus::Blocked);
    }

    #[test]
    fn blocks_dangerous_command() {
        let result = run_terminal_command(request("sudo rm -rf /"));
        assert_eq!(result.status, TerminalCommandStatus::Blocked);
        assert!(!result.safety.allowed);
    }
}
