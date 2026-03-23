//! Compiler-in-the-loop verification.
//!
//! After tools modify files, runs the project's build/check command
//! and feeds diagnostics back into the agent's context. The compiler
//! is the reward model — build errors surface immediately so the agent
//! can fix them in the same turn.

use std::path::Path;
use std::process::Command;

use tracing::info;

/// Detect the project type and run the appropriate build check.
/// Returns (success, diagnostics) where success is true if the
/// build passed and diagnostics is a list of error/warning strings.
pub fn check_project() -> (bool, Vec<String>) {
    // Detect project type from files in cwd
    if Path::new("Cargo.toml").exists() {
        check_rust()
    } else if Path::new("go.mod").exists() {
        check_go()
    } else if Path::new("tsconfig.json").exists() || Path::new("package.json").exists() {
        check_typescript()
    } else if Path::new("pyproject.toml").exists()
        || Path::new("setup.py").exists()
        || Path::new("requirements.txt").exists()
    {
        check_python()
    } else if Path::new("Makefile").exists() || Path::new("CMakeLists.txt").exists() {
        check_c()
    } else {
        (true, vec![])
    }
}

/// Check a single file for syntax errors (lightweight, for per-file feedback).
pub fn check_file(path: &str) -> Vec<String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "rs" => check_rust().1,
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" => check_c().1,
        "py" => check_python_file(path),
        "ts" | "tsx" | "js" | "jsx" => check_typescript().1,
        "go" => check_go().1,
        _ => vec![],
    }
}

/// Returns true if any of the given tool names modify files.
pub fn modifies_files(tool_names: &[&str]) -> bool {
    tool_names.iter().any(|t| {
        matches!(
            *t,
            "write_file" | "edit_file" | "shell"
        )
    })
}

fn check_rust() -> (bool, Vec<String>) {
    info!("running cargo check");
    let output = Command::new("cargo")
        .args(["check", "--message-format=short", "--color=never"])
        .output();
    parse_build_output(output)
}

fn check_go() -> (bool, Vec<String>) {
    info!("running go vet");
    let output = Command::new("go")
        .args(["vet", "./..."])
        .output();
    parse_build_output(output)
}

fn check_typescript() -> (bool, Vec<String>) {
    info!("running tsc --noEmit");
    let output = Command::new("npx")
        .args(["tsc", "--noEmit", "--pretty", "false"])
        .output();
    parse_build_output(output)
}

fn check_python() -> (bool, Vec<String>) {
    // Try ruff first (fast), fall back to pyflakes
    info!("running python linter");
    let output = Command::new("ruff")
        .args(["check", "."])
        .output()
        .or_else(|_| Command::new("python3").args(["-m", "pyflakes", "."]).output());
    parse_build_output(output)
}

fn check_python_file(path: &str) -> Vec<String> {
    let output = Command::new("python3")
        .args(["-m", "py_compile", path])
        .output();
    parse_build_output(output).1
}

fn check_c() -> (bool, Vec<String>) {
    info!("running make/cmake check");
    // Try make first, then cmake
    let output = Command::new("make")
        .args(["-n"]) // dry run to check for errors
        .output();
    if output.is_err() {
        return (true, vec![]);
    }
    // Actually run the build
    let output = Command::new("make").output();
    parse_build_output(output)
}

fn parse_build_output(output: std::io::Result<std::process::Output>) -> (bool, Vec<String>) {
    match output {
        Ok(out) => {
            let success = out.status.success();
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{stderr}{stdout}");
            let diags: Vec<String> = combined
                .lines()
                .filter(|l| {
                    let lower = l.to_lowercase();
                    lower.contains("error") || lower.contains("warning")
                })
                .take(20)
                .map(|l| l.trim().to_string())
                .collect();
            (success, diags)
        }
        Err(_) => (true, vec![]), // tool not found = skip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifies_files() {
        assert!(modifies_files(&["edit_file"]));
        assert!(modifies_files(&["write_file"]));
        assert!(modifies_files(&["shell"]));
        assert!(!modifies_files(&["read_file"]));
        assert!(!modifies_files(&["grep"]));
    }

    #[test]
    fn test_check_file_unknown_ext() {
        let diags = check_file("/tmp/test.xyz");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_empty_output() {
        let (success, diags) = parse_build_output(Ok(std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: vec![],
            stderr: vec![],
        }));
        assert!(diags.is_empty());
        // ExitStatus::default() is success on most platforms
        assert!(success);
    }
}
