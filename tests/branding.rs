use std::path::PathBuf;
use std::process::Command;

const COMMANDS: [(&str, &str); 2] = [
    ("navifast", env!("CARGO_BIN_EXE_navifast")),
    ("fastpotify", env!("CARGO_BIN_EXE_fastpotify")),
];

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("navifast-rename-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn both_commands_report_their_name_and_pass_the_update_version_check() {
    for (name, binary) in COMMANDS {
        let output = Command::new(binary).arg("--version").output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            format!("{name} {}", env!("CARGO_PKG_VERSION"))
        );
        let help = Command::new(binary).arg("--help").output().unwrap();
        assert!(help.status.success());
        assert!(
            String::from_utf8(help.stdout)
                .unwrap()
                .contains(&format!("Usage: {name}"))
        );
        // The updater's version probe accepts the slug or a legacy name.
        let config = navifast::updates::CONFIG;
        assert!(name == config.slug || config.legacy_names.contains(&name));
    }
}

#[cfg(unix)]
#[test]
fn the_linux_package_alias_reports_navifast_without_copying_the_app() {
    let scratch = Scratch::new();
    let alias = scratch.0.join("navifast");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fastpotify"), &alias).unwrap();
    let output = Command::new(alias).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("navifast {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn existing_preferences_and_historical_fields_survive() {
    use navifast::settings::{Settings, ThemeChoice};
    let scratch = Scratch::new();
    let path = scratch.0.join("settings.json");
    let saved: Settings = serde_json::from_value(serde_json::json!({
        "device_name":"Living room", "theme":"light", "volume":37,
        "pinned_contexts":["spotify:playlist:123", "fastpotify:liked-songs"]
    }))
    .unwrap();
    saved.save(&path);
    let loaded = Settings::load(&path);
    assert_eq!(loaded, saved);
    assert_eq!(loaded.theme, ThemeChoice::Light);
}

#[cfg(target_os = "linux")]
#[test]
fn both_commands_forward_controls_to_the_existing_instance_on_a_private_bus() {
    use navifast::single_instance::{ControlCommand, Outcome};

    const CHILD: &str = "NAVIFAST_RENAME_PRIVATE_BUS";
    if std::env::var_os(CHILD).is_none() {
        // A clean build (including Nix) need not have /etc/dbus-1/session.conf.
        // Own the bus configuration too, without loading desktop services.
        let scratch = Scratch::new();
        let config = scratch.0.join("session.conf");
        std::fs::write(
            &config,
            r#"<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow own="*"/>
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
  </policy>
</busconfig>"#,
        )
        .unwrap();
        let result = Command::new("dbus-run-session")
            .arg("--config-file")
            .arg(config)
            .args(["--"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "both_commands_forward_controls_to_the_existing_instance_on_a_private_bus",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .expect("the Linux test environment needs dbus-run-session");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }

    for (_, binary) in COMMANDS {
        let result = Command::new(binary).arg("reload-themes").output().unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&result.stderr).contains("not running"));
        let result = Command::new(binary).arg("like").output().unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("not running"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let Outcome::Only(guard) =
        navifast::single_instance::acquire_with_wake(std::sync::Arc::new(|| {}))
    else {
        panic!("the private bus must start without another instance");
    };
    let commands = guard.commands();
    for (_, binary) in COMMANDS {
        let result = Command::new(binary).arg("reload-themes").output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            std::mem::take(&mut *commands.lock().unwrap()),
            vec![ControlCommand::ReloadThemes],
            "a reload only asks for themes, never Show"
        );
        let result = Command::new(binary).arg("like").output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            std::mem::take(&mut *commands.lock().unwrap()),
            vec![ControlCommand::ToggleSaved],
            "like only toggles the saved state, never Show"
        );
    }
}
