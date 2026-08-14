use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn main() {
    println!("cargo:rerun-if-env-changed=SOLVER_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=SOLVER_BUILT_AT");

    let git_commit = env::var("SOLVER_GIT_COMMIT")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| command_output("git", &["rev-parse", "HEAD"]));
    let built_at = env::var("SOLVER_BUILT_AT")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| command_output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]));

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let generated = format!(
        "pub const GIT_COMMIT: Option<&str> = {};\npub const BUILT_AT: Option<&str> = {};\n",
        option_literal(git_commit.as_deref()),
        option_literal(built_at.as_deref()),
    );
    fs::write(out_dir.join("solver_build_info.rs"), generated)
        .expect("write solver build metadata");
}

fn option_literal(value: Option<&str>) -> String {
    value
        .map(|value| format!("Some({value:?})"))
        .unwrap_or_else(|| "None".to_string())
}
