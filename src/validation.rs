// Validation module for command and working directory validation

use regex::RegexBuilder;
use std::path::Path;

/// Validate a shell command for safety and basic constraints.
///
/// Returns `Ok(())` if the command is valid, or `Err(message)` if not.
pub fn validate_command(command: &str) -> Result<(), String> {
    if command.is_empty() || command.trim().is_empty() {
        return Err("Command cannot be empty".to_string());
    }

    if command.len() > 10000 {
        return Err("Command too long (max 10000 characters)".to_string());
    }

    let dangerous_patterns: &[&str] = &[
        r"rm\s+-rf\s+/",
        r":\(\)\{\s*:\|:\s*&\s*\};:",
        r"dd\s+if=/dev/zero",
    ];

    for pattern_str in dangerous_patterns {
        let re = RegexBuilder::new(pattern_str)
            .case_insensitive(true)
            .build()
            .unwrap();
        if re.is_match(command) {
            return Err(format!(
                "Command contains potentially dangerous pattern: {}",
                pattern_str
            ));
        }
    }

    Ok(())
}

/// Validate and resolve a working directory path.
///
/// Returns `Ok(None)` if `cwd` is `None`, `Ok(Some(resolved_path))` if valid,
/// or `Err(message)` if invalid.
pub fn validate_cwd(cwd: Option<&str>) -> Result<Option<String>, String> {
    let cwd = match cwd {
        None => return Ok(None),
        Some(s) => s,
    };

    if cwd.trim().is_empty() {
        return Err("Working directory cannot be empty string".to_string());
    }

    let path = Path::new(cwd);
    if !path.exists() {
        return Err(format!("Working directory does not exist: {}", cwd));
    }
    if !path.is_dir() {
        return Err(format!("Working directory is not a directory: {}", cwd));
    }
    match path.canonicalize() {
        Ok(resolved) => Ok(Some(resolved.to_string_lossy().into_owned())),
        Err(e) => Err(format!("Invalid working directory: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_command tests ---

    #[test]
    fn test_empty_command() {
        assert_eq!(
            validate_command(""),
            Err("Command cannot be empty".to_string())
        );
        assert_eq!(
            validate_command("   "),
            Err("Command cannot be empty".to_string())
        );
        assert_eq!(
            validate_command("\t\n"),
            Err("Command cannot be empty".to_string())
        );
    }

    #[test]
    fn test_command_too_long() {
        // 10001 chars should fail
        let long = "a".repeat(10001);
        assert_eq!(
            validate_command(&long),
            Err("Command too long (max 10000 characters)".to_string())
        );
    }

    #[test]
    fn test_command_at_limit() {
        // exactly 10000 chars should pass
        let at_limit = "a".repeat(10000);
        assert!(validate_command(&at_limit).is_ok());
    }

    #[test]
    fn test_dangerous_rm_rf() {
        // rm -rf / is dangerous
        assert_eq!(
            validate_command("rm -rf /"),
            Err("Command contains potentially dangerous pattern: rm\\s+-rf\\s+/".to_string())
        );
        // rm -rf /home is also caught (pattern matches anything starting with /)
        assert_eq!(
            validate_command("rm -rf /home"),
            Err("Command contains potentially dangerous pattern: rm\\s+-rf\\s+/".to_string())
        );
        // rm file.txt is safe
        assert!(validate_command("rm file.txt").is_ok());
        // rm -rf ./local is safe (no leading slash)
        assert!(validate_command("rm -rf ./local").is_ok());
    }

    #[test]
    fn test_dangerous_fork_bomb() {
        assert_eq!(
            validate_command(":(){ :|: & };:"),
            Err(
                "Command contains potentially dangerous pattern: :\\(\\)\\{\\s*:\\|:\\s*&\\s*\\};:"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_dangerous_dd() {
        assert_eq!(
            validate_command("dd if=/dev/zero of=/dev/sda"),
            Err("Command contains potentially dangerous pattern: dd\\s+if=/dev/zero".to_string())
        );
    }

    #[test]
    fn test_dangerous_case_insensitive() {
        // Case-insensitive: RM -RF / should be caught
        assert_eq!(
            validate_command("RM -RF /"),
            Err("Command contains potentially dangerous pattern: rm\\s+-rf\\s+/".to_string())
        );
        // Mixed case
        assert_eq!(
            validate_command("Rm -Rf /tmp"),
            Err("Command contains potentially dangerous pattern: rm\\s+-rf\\s+/".to_string())
        );
    }

    #[test]
    fn test_safe_commands_pass() {
        assert!(validate_command("echo hello").is_ok());
        assert!(validate_command("ls -la").is_ok());
        // rm -rf /tmp/mydir IS caught by the pattern (matches rm -rf /anything).
        assert!(validate_command("git commit -m 'message'").is_ok());
        assert!(validate_command("cargo test").is_ok());
    }

    // --- validate_cwd tests ---

    #[test]
    fn test_cwd_none() {
        assert_eq!(validate_cwd(None), Ok(None));
    }

    #[test]
    fn test_cwd_empty() {
        assert_eq!(
            validate_cwd(Some("")),
            Err("Working directory cannot be empty string".to_string())
        );
        assert_eq!(
            validate_cwd(Some("   ")),
            Err("Working directory cannot be empty string".to_string())
        );
        assert_eq!(
            validate_cwd(Some("\t")),
            Err("Working directory cannot be empty string".to_string())
        );
    }

    #[test]
    fn test_cwd_nonexistent() {
        let path = "/nonexistent/path/does/not/exist";
        assert_eq!(
            validate_cwd(Some(path)),
            Err(format!("Working directory does not exist: {}", path))
        );
    }

    #[test]
    fn test_cwd_is_file() {
        let tmp_file = tempfile::NamedTempFile::new().expect("failed to create temp file");
        let path_str = tmp_file.path().to_str().expect("non-utf8 path").to_string();
        assert_eq!(
            validate_cwd(Some(&path_str)),
            Err(format!(
                "Working directory is not a directory: {}",
                path_str
            ))
        );
    }

    #[test]
    fn test_cwd_valid() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let path_str = tmp_dir.path().to_str().expect("non-utf8 path").to_string();
        let result = validate_cwd(Some(&path_str));
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.is_some());
        // Resolved path should be Some(string)
        let resolved_path = resolved.unwrap();
        assert!(!resolved_path.is_empty());
    }
}
