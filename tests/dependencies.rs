//! The production and Rune preview graphs must exclude the comparison framework.
use std::process::Command;

#[test]
fn normal_and_rune_preview_builds_exclude_fastframe() {
    for features in [None, Some("navidrome-demo")] {
        let mut command = Command::new(env!("CARGO"));
        command.current_dir(env!("CARGO_MANIFEST_DIR")).args([
            "tree",
            "--locked",
            "--offline",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
        ]);
        if let Some(features) = features {
            command.args(["--features", features]);
        }
        let output = command.output().expect("read the dependency graph");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let graph = String::from_utf8(output.stdout).unwrap();
        assert!(
            !graph.lines().any(|line| line.starts_with("fastframe-")),
            "{features:?} includes Fastframe"
        );
    }
}
