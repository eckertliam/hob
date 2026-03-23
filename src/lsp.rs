//! Lightweight LSP client for post-edit diagnostics.
//!
//! After edit_file or write_file modifies a file, we can optionally
//! check for errors by running the appropriate language server.
//! This is a simple one-shot approach: we don't maintain a persistent
//! LSP connection, just run a quick check command.

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Run a quick diagnostic check on a file after editing.
/// Returns a list of diagnostic messages, or empty if no errors found
/// or no suitable checker is available.
pub fn check_file(path: &str) -> Vec<String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "rs" => check_rust(path),
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" => check_clang(path),
        "py" => check_python(path),
        "ts" | "tsx" | "js" | "jsx" => check_typescript(path),
        "go" => check_go(path),
        _ => vec![],
    }
}

/// Check Rust file using cargo check.
fn check_rust(_path: &str) -> Vec<String> {
    let output = Command::new("cargo")
        .args(["check", "--message-format=short"])
        .output();

    parse_compiler_output(output)
}

/// Check C/C++ using clang.
fn check_clang(path: &str) -> Vec<String> {
    let output = Command::new("clang")
        .args(["-fsyntax-only", "-fdiagnostics-format=clang", path])
        .output();

    if output.is_err() {
        // Try clang++ for C++ files
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "cpp" | "cc" | "cxx" | "hpp") {
            let output = Command::new("clang++")
                .args(["-fsyntax-only", path])
                .output();
            return parse_compiler_output(output);
        }
    }

    parse_compiler_output(output)
}

/// Check Python using python -m py_compile.
fn check_python(path: &str) -> Vec<String> {
    let output = Command::new("python3")
        .args(["-m", "py_compile", path])
        .output();

    parse_compiler_output(output)
}

/// Check TypeScript using tsc --noEmit.
fn check_typescript(path: &str) -> Vec<String> {
    let output = Command::new("npx")
        .args(["tsc", "--noEmit", "--pretty", "false", path])
        .output();

    parse_compiler_output(output)
}

/// Check Go using go vet.
fn check_go(_path: &str) -> Vec<String> {
    let output = Command::new("go")
        .args(["vet", "./..."])
        .output();

    parse_compiler_output(output)
}

fn parse_compiler_output(output: std::io::Result<std::process::Output>) -> Vec<String> {
    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{stderr}{stdout}");
            combined
                .lines()
                .filter(|l| {
                    let lower = l.to_lowercase();
                    lower.contains("error") || lower.contains("warning")
                })
                .take(20)
                .map(|l| l.trim().to_string())
                .collect()
        }
        Err(_) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_unknown_extension_returns_empty() {
        let diags = check_file("/tmp/test.xyz");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_compiler_output_empty() {
        let result = parse_compiler_output(Ok(std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: vec![],
            stderr: vec![],
        }));
        assert!(result.is_empty());
    }
}
