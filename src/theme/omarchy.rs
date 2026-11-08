//! Omarchy theme setup and migration of the hook installed by older packages.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Rewrites the user's Omarchy hook when it is exactly the one a
/// Fastpotify-era package installed, which copied palettes into
/// `fastpotify/themes`. Setup installs the packaged hook only
/// where none exists, so without this the old one would stay forever. A hook
/// the user edited is left alone.
pub(super) fn upgrade_legacy_hook() {
    let Some(assets) = packaged_assets() else {
        return;
    };
    let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) else {
        return;
    };
    if let Err(error) = upgrade(&assets, &home) {
        log::warn!("unable to upgrade the Omarchy theme hook: {error}");
    }
}

/// `<prefix>/share/navifast/omarchy` beside the running `<prefix>/bin`.
fn packaged_assets() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    Some(
        executable
            .parent()?
            .parent()?
            .join("share/navifast/omarchy"),
    )
}

fn upgrade(assets: &Path, home: &Path) -> io::Result<()> {
    // Only an installed package on a configured Omarchy desktop, as when
    // the hook was installed. Cargo builds and other desktops do nothing.
    if !assets.is_dir()
        || !home.join(".config/omarchy").is_dir()
        || !home.join(".local/state/omarchy/current/theme").is_dir()
    {
        return Ok(());
    }
    let hook_path = home.join(".config/omarchy/hooks/theme-set.d/navifast-theme");
    if !fs::symlink_metadata(&hook_path).is_ok_and(|metadata| metadata.is_file()) {
        return Ok(());
    }
    let hook = read_small(&assets.join("navifast-theme"))?;
    let previous = hook.replace("/navifast/themes", "/fastpotify/themes");
    if read_small(&hook_path)? != previous {
        return Ok(());
    }
    let temporary = hook_path.with_extension(format!("migration-{}.tmp", rand::random::<u64>()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(&temporary)?;
    file.write_all(hook.as_bytes())?;
    file.sync_all()?;
    fs::rename(temporary, &hook_path)
}

fn read_small(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err(io::Error::other("Omarchy theme file exceeds 64 KiB"));
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

#[derive(Clone)]
pub(super) struct Setup {
    assets: PathBuf,
    home: PathBuf,
}

impl Setup {
    pub(super) fn discover() -> Option<Self> {
        let executable = std::env::current_exe().ok()?;
        Some(Self {
            assets: executable
                .parent()?
                .parent()?
                .join("share/navifast/omarchy"),
            home: directories::BaseDirs::new()?.home_dir().to_path_buf(),
        })
    }

    pub(super) fn available(&self) -> bool {
        self.assets.is_dir()
            && self.home.join(".config/omarchy").is_dir()
            && self
                .home
                .join(".local/state/omarchy/current/theme")
                .is_dir()
    }

    pub(super) fn install(&self, themes: &Path) -> io::Result<()> {
        // Keep the old hook and profile together during an updater trial.
        if themes == crate::paths::AppDirs::legacy().config.join("themes") {
            return Ok(());
        }
        let config = self.home.join(".config/omarchy");
        let current = self.home.join(".local/state/omarchy/current/theme");
        // Only an installed package on a configured Omarchy desktop opts in.
        // Cargo builds, portable binaries and other desktops do nothing.
        if !self.available() {
            return Ok(());
        }
        let template = read_small(&self.assets.join("navifast.json.tpl"))?;
        let hook = read_small(&self.assets.join("navifast-theme"))?;
        let hook_path = config.join("hooks/theme-set.d/navifast-theme");
        // Upgrade only our exact previous hook. Preserve any user edits.
        let previous = hook.replace("/navifast/themes", "/fastpotify/themes");
        if fs::symlink_metadata(&hook_path).is_ok_and(|metadata| metadata.is_file())
            && read_small(&hook_path).is_ok_and(|installed| installed == previous)
        {
            let temporary =
                hook_path.with_extension(format!("migration-{}.tmp", rand::random::<u64>()));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o755)
                .open(&temporary)?;
            file.write_all(hook.as_bytes())?;
            file.sync_all()?;
            fs::rename(temporary, &hook_path)?;
        }
        let template_path = config.join("themed/navifast.json.tpl");
        create_only(&template_path, template.as_bytes(), 0o644)?;
        create_only(&hook_path, hook.as_bytes(), 0o755)?;

        let destination = themes.join("omarchy.json");
        if fs::symlink_metadata(&destination).is_ok() {
            return Ok(());
        }
        // Seed the current palette without reapplying the desktop theme.
        // Future changes use Omarchy's own template renderer and installed hook.
        let rendered = current.join("navifast.json");
        let palette = if rendered.is_file() {
            read_small(&rendered)?
        } else {
            let output = Command::new("omarchy-theme-color")
                .arg("--file")
                .arg(current.join("colors.toml"))
                .arg("--all")
                .stdin(Stdio::null())
                .output()?;
            if !output.status.success() || output.stdout.len() > 64 * 1024 {
                return Err(io::Error::other(
                    "Omarchy's current colors could not be read",
                ));
            }
            let colors = std::str::from_utf8(&output.stdout).map_err(io::Error::other)?;
            render_seed(&read_small(&template_path)?, colors)?
        };
        super::custom::parse_palette(&palette).map_err(io::Error::other)?;
        create_only(&destination, palette.as_bytes(), 0o644)
    }
}

fn create_only(destination: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".navifast-setup-{:016x}", rand::random::<u64>()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, destination) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            result => result,
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}

/// Resolve the color placeholders used by our shipped JSON template for its
/// first use. Omarchy itself remains responsible for later theme changes.
fn render_seed(template: &str, colors: &str) -> io::Result<String> {
    let colors: BTreeMap<_, _> = colors
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let mut output = String::new();
    let mut rest = template;
    while let Some((before, token)) = rest.split_once("{{") {
        output.push_str(before);
        let (token, after) = token
            .split_once("}}")
            .ok_or_else(|| io::Error::other("incomplete palette placeholder"))?;
        let words: Vec<_> = token.split_whitespace().collect();
        let get = |key: &str| {
            colors
                .get(key)
                .copied()
                .ok_or_else(|| io::Error::other("missing Omarchy color"))
        };
        match words.as_slice() {
            [key] => output.push_str(get(key)?),
            ["mix", from, to, percent] => {
                let percent: u32 = percent
                    .strip_suffix('%')
                    .and_then(|p| p.parse().ok())
                    .filter(|p| *p <= 100)
                    .ok_or_else(|| io::Error::other("unsupported palette mix"))?;
                let rgb = |value: &str| -> io::Result<u32> {
                    let hex = value
                        .strip_prefix('#')
                        .filter(|s| s.len() == 6)
                        .ok_or_else(|| io::Error::other("expected an RGB color"))?;
                    u32::from_str_radix(hex, 16).map_err(io::Error::other)
                };
                let (from, to) = (rgb(get(from)?)?, rgb(get(to)?)?);
                output.push('#');
                for shift in [16, 8, 0] {
                    let value = (((from >> shift) & 255) * (100 - percent)
                        + ((to >> shift) & 255) * percent
                        + 50)
                        / 100;
                    output.push_str(&format!("{value:02x}"));
                }
            }
            _ => return Err(io::Error::other("unsupported palette placeholder")),
        }
        rest = after;
    }
    output.push_str(rest);
    super::custom::parse_palette(&output).map_err(io::Error::other)?;
    Ok(output)
}

pub(super) struct ThemeWatch {
    _watcher: notify::RecommendedWatcher,
    changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl ThemeWatch {
    pub(super) fn start(local: &Path, setup: Option<&Setup>, wake: rune::Waker) -> Option<Self> {
        use notify::{RecursiveMode, Watcher};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let changed = Arc::new(AtomicBool::new(false));
        let dirty = changed.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event
                    .is_ok_and(|e| e.kind.is_create() || e.kind.is_modify() || e.kind.is_remove())
                    && !dirty.swap(true, Ordering::AcqRel)
                {
                    wake();
                }
            })
            .ok()?;
        watcher.watch(local, RecursiveMode::NonRecursive).ok()?;
        if let Some(setup) = setup.filter(|s| s.available()) {
            watcher
                .watch(
                    &setup.home.join(".local/state/omarchy/current"),
                    RecursiveMode::Recursive,
                )
                .ok()?;
        }
        Some(Self {
            _watcher: watcher,
            changed,
        })
    }

    pub(super) fn take_changed(&self) -> bool {
        self.changed
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const HOOK: &str = include_str!("../../contrib/omarchy/navifast-theme");

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("navifast omarchy setup {}", rand::random::<u64>()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_the_exact_fastpotify_hook_is_upgraded() {
        let root = Scratch::new();
        let assets = root.0.join("package/share/navifast/omarchy");
        let home = root.0.join("user");
        let hooks = home.join(".config/omarchy/hooks/theme-set.d");
        let hook = hooks.join("navifast-theme");
        let legacy = HOOK.replace("/navifast/themes", "/fastpotify/themes");
        assert_ne!(legacy, HOOK);

        // No package, no Omarchy: nothing is touched.
        upgrade(&assets, &home).unwrap();
        assert!(!home.exists());

        for path in [
            &assets,
            &hooks,
            &home.join(".local/state/omarchy/current/theme"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(assets.join("navifast-theme"), HOOK).unwrap();
        // No hook installed: first-run setup handles installation.
        upgrade(&assets, &home).unwrap();
        assert!(!hook.exists());

        fs::write(&hook, &legacy).unwrap();
        upgrade(&assets, &home).unwrap();
        assert_eq!(fs::read_to_string(&hook).unwrap(), HOOK);
        assert_ne!(fs::metadata(&hook).unwrap().permissions().mode() & 0o100, 0);
        assert_eq!(fs::read_dir(&hooks).unwrap().count(), 1);

        fs::write(&hook, "user hook").unwrap();
        upgrade(&assets, &home).unwrap();
        assert_eq!(fs::read_to_string(&hook).unwrap(), "user hook");
    }
}
