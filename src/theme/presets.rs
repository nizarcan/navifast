//! The shipped palettes become editable files on first launch. The record
//! distinguishes a user deletion from a palette introduced by a newer build.
use super::custom::{CustomTheme, Loaded, discover, parse_palette};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::Path,
};

macro_rules! palettes {
    ($($name:literal),+ $(,)?) => { &[$(($name, include_str!(concat!("../../assets/themes/", $name)))),+] };
}
const PALETTES: &[(&str, &str)] = palettes![
    "Catppuccin Latte.json",
    "Catppuccin.json",
    "Nord.json",
    "Ristretto.json",
    "Rose Pine Dawn.json",
    "Rose Pine Moon.json",
    "Rose Pine.json",
    "Tokyo Night.json",
];

fn install(directory: &Path) -> io::Result<BTreeSet<String>> {
    fs::create_dir_all(directory)?;
    let record = directory.join(".installed-palettes");
    let mut installed: BTreeSet<String> = match fs::read_to_string(&record) {
        Ok(text) => text.lines().map(str::to_owned).collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => return Err(error),
    };
    let previous = installed.len();
    for &(name, json) in PALETTES {
        if installed.contains(name) {
            continue;
        }
        match install_palette(directory, name, json) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        installed.insert(name.into());
    }
    if installed.len() != previous {
        let path = directory.join(format!(
            ".installed-palettes-{:016x}.tmp",
            rand::random::<u64>()
        ));
        let result = (|| {
            let mut file = fs::File::create_new(&path)?;
            for name in &installed {
                writeln!(file, "{name}")?;
            }
            file.sync_all()?;
            drop(file);
            crate::util::replace_file(&path, &record)
        })();
        let _ = fs::remove_file(path);
        result?;
    }
    Ok(installed)
}

/// Publish a complete file without replacing a concurrently created user file.
fn install_palette(directory: &Path, name: &str, json: &str) -> io::Result<()> {
    let temporary = directory.join(format!(".palette-{:016x}.tmp", rand::random::<u64>()));
    let result = (|| {
        let mut file = fs::File::create_new(&temporary)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::hard_link(&temporary, directory.join(name))
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub(super) fn load(directory: &Path, selected: Option<&str>) -> Loaded {
    let installed = install(directory).unwrap_or_else(|error| {
        log::warn!("unable to install the theme presets: {error}");
        fs::read_to_string(directory.join(".installed-palettes"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    });
    let mut loaded = discover(directory, selected);
    for &(name, json) in PALETTES {
        if installed.contains(name) || directory.join(name).exists() {
            continue;
        }
        loaded.themes.push(CustomTheme {
            filename: name.into(),
            palette: parse_palette(json).expect("bundled palette"),
        });
    }
    loaded.themes.sort_by(|a, b| a.filename.cmp(&b.filename));
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publishing_keeps_existing_files_and_cleans_up_after_failure() {
        let path = std::env::temp_dir().join(format!(
            "navifast-palette-write-{:016x}",
            rand::random::<u64>()
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("custom.json"), b"original").unwrap();
        assert_eq!(
            install_palette(&path, "custom.json", "replacement")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(path.join("custom.json")).unwrap(), b"original");
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
        install_palette(&path, "new.json", "complete").unwrap();
        assert_eq!(fs::read(path.join("new.json")).unwrap(), b"complete");
        assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn reload_respects_customized_and_deleted_presets() {
        let path =
            std::env::temp_dir().join(format!("navifast-presets-{:016x}", rand::random::<u64>()));
        assert_eq!(load(&path, None).themes.len(), 8);
        fs::remove_file(path.join("Nord.json")).unwrap();
        fs::write(path.join("Ristretto.json"), r#"{"base":"light"}"#).unwrap();
        let result = load(&path, None);
        assert_eq!(result.themes.len(), 7);
        assert!(
            !result
                .themes
                .iter()
                .find(|p| p.filename == "Ristretto.json")
                .unwrap()
                .palette
                .dark
        );
        fs::remove_dir_all(path).unwrap();
    }
}
