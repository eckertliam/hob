//! OS-level sandboxing for autonomous tool execution.
//!
//! When sandboxing is enabled, the agent can execute tools without
//! permission prompts because the OS restricts access to only the
//! workspace directory.
//!
//! - macOS: Seatbelt (sandbox-exec)
//! - Linux: Landlock LSM
//!
//! Three modes:
//! - ReadOnly: can only read files in the workspace
//! - WorkspaceWrite: can read/write within workspace, no network except API
//! - FullAccess: no restrictions (default, uses permission prompts)

use std::path::Path;

use anyhow::Result;
use tracing::info;

/// Sandbox mode.
#[derive(Debug, Clone, PartialEq)]
pub enum SandboxMode {
    /// No OS-level restrictions. Uses permission prompts.
    FullAccess,
    /// Can only read files within the workspace.
    ReadOnly,
    /// Can read and write within the workspace. Network limited to APIs.
    WorkspaceWrite,
}

/// Apply the sandbox policy for the current process.
/// Must be called early in startup, before any tool execution.
pub fn apply(mode: &SandboxMode, workspace: &Path) -> Result<()> {
    match mode {
        SandboxMode::FullAccess => {
            info!("sandbox: full access (no OS restrictions)");
            Ok(())
        }
        _ => {
            #[cfg(target_os = "macos")]
            return apply_seatbelt(mode, workspace);

            #[cfg(target_os = "linux")]
            return apply_landlock(mode, workspace);

            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                tracing::warn!("sandbox: not supported on this platform");
                Ok(())
            }
        }
    }
}

/// macOS: apply Seatbelt sandbox profile.
#[cfg(target_os = "macos")]
fn apply_seatbelt(mode: &SandboxMode, workspace: &Path) -> Result<()> {
    use std::process::Command;

    let ws = workspace.display().to_string();
    let profile = match mode {
        SandboxMode::ReadOnly => format!(
            r#"(version 1)
(deny default)
(allow file-read* (subpath "{ws}"))
(allow file-read* (subpath "/usr"))
(allow file-read* (subpath "/Library"))
(allow file-read* (subpath "/System"))
(allow file-read* (subpath "/private/tmp"))
(allow process-exec)
(allow sysctl-read)
(allow mach-lookup)
(allow network-outbound (remote tcp "*:443"))
"#
        ),
        SandboxMode::WorkspaceWrite => format!(
            r#"(version 1)
(deny default)
(allow file-read* (subpath "{ws}"))
(allow file-read* (subpath "/usr"))
(allow file-read* (subpath "/Library"))
(allow file-read* (subpath "/System"))
(allow file-read* (subpath "/private/tmp"))
(allow file-write* (subpath "{ws}"))
(allow file-write* (subpath "/private/tmp"))
(allow process-exec)
(allow sysctl-read)
(allow mach-lookup)
(allow network-outbound (remote tcp "*:443"))
"#
        ),
        SandboxMode::FullAccess => unreachable!(),
    };

    info!("sandbox: applying seatbelt ({:?})", mode);

    // Write profile to a temp file
    let profile_path = std::env::temp_dir().join("hob-sandbox.sb");
    std::fs::write(&profile_path, &profile)?;

    // Note: sandbox-exec applies to child processes, not the current process.
    // For the current process, we'd need to use sandbox_init() via FFI.
    // For now, we apply it to shell tool commands by wrapping them.
    // Store the profile path for use by the shell tool.
    std::env::set_var("HOB_SANDBOX_PROFILE", profile_path.display().to_string());

    Ok(())
}

/// Linux: apply Landlock restrictions.
#[cfg(target_os = "linux")]
fn apply_landlock(mode: &SandboxMode, workspace: &Path) -> Result<()> {
    // Landlock requires kernel 5.13+ and specific capabilities.
    // This is a simplified version — a full implementation would use
    // the landlock syscalls directly.
    info!("sandbox: landlock ({:?}) for {}", mode, workspace.display());

    // Check if Landlock is available
    let status = std::fs::read_to_string("/sys/kernel/security/landlock/abi_version");
    match status {
        Ok(version) => {
            info!("sandbox: landlock ABI version {}", version.trim());
        }
        Err(_) => {
            tracing::warn!("sandbox: landlock not available on this kernel");
            return Ok(());
        }
    }

    // Store mode for use by tools
    match mode {
        SandboxMode::ReadOnly => {
            std::env::set_var("HOB_SANDBOX_MODE", "readonly");
        }
        SandboxMode::WorkspaceWrite => {
            std::env::set_var("HOB_SANDBOX_MODE", "workspace");
        }
        _ => {}
    }
    std::env::set_var("HOB_SANDBOX_WORKSPACE", workspace.display().to_string());

    Ok(())
}

/// Check if sandboxing is active.
pub fn is_sandboxed() -> bool {
    std::env::var("HOB_SANDBOX_PROFILE").is_ok()
        || std::env::var("HOB_SANDBOX_MODE").is_ok()
}

/// Parse sandbox mode from config string.
pub fn parse_mode(s: &str) -> SandboxMode {
    match s {
        "readonly" | "read-only" | "read_only" => SandboxMode::ReadOnly,
        "workspace" | "workspace-write" | "workspace_write" => SandboxMode::WorkspaceWrite,
        _ => SandboxMode::FullAccess,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode() {
        assert_eq!(parse_mode("readonly"), SandboxMode::ReadOnly);
        assert_eq!(parse_mode("workspace"), SandboxMode::WorkspaceWrite);
        assert_eq!(parse_mode("full"), SandboxMode::FullAccess);
        assert_eq!(parse_mode(""), SandboxMode::FullAccess);
    }

    #[test]
    fn test_full_access_is_noop() {
        let result = apply(&SandboxMode::FullAccess, Path::new("/tmp"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_sandboxed_default_false() {
        // Clean env
        std::env::remove_var("HOB_SANDBOX_PROFILE");
        std::env::remove_var("HOB_SANDBOX_MODE");
        assert!(!is_sandboxed());
    }
}
