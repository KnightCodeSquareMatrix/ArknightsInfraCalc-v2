use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn plan_help_flags_fail_before_loading_inputs_or_writing_profile() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let missing_operbox = PathBuf::from(format!("target/plan-help-must-not-read-{nonce}.json"));
    let profile_out = PathBuf::from(format!("target/plan-help-must-not-write-{nonce}.json"));
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    for flag in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_infra-cli"))
            .current_dir(&workspace_root)
            .args([
                "plan",
                flag,
                "--operbox",
                missing_operbox.to_str().unwrap(),
                "--profile-out",
                profile_out.to_str().unwrap(),
            ])
            .output()
            .unwrap();

        assert!(!output.status.success(), "{flag} should be rejected");
        assert!(
            output.stdout.is_empty(),
            "{flag} must not print help output"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("plan does not support"),
            "unexpected stderr for {flag}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !profile_out.exists(),
            "{flag} unexpectedly wrote {}",
            profile_out.display()
        );
    }

    let output = Command::new(env!("CARGO_BIN_EXE_infra-cli"))
        .current_dir(&workspace_root)
        .args([
            "plan",
            "--maa-title",
            "-h",
            "--operbox",
            missing_operbox.to_str().unwrap(),
            "--profile-out",
            profile_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("plan does not support"),
        "-h used as a --maa-title value must not be treated as a help flag"
    );
    assert!(!profile_out.exists());
}
