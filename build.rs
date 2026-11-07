//! Compile translations and embed native Windows resources.

#[path = "build_support/catalogs.rs"]
mod catalogs;

fn main() {
    println!("cargo:rerun-if-changed=build_support/catalogs.rs");
    println!("cargo:rerun-if-changed=assets/i18n");
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let mut modules = Vec::new();
    for entry in std::fs::read_dir("assets/i18n").expect("translation directory") {
        let path = entry.expect("translation entry").path();
        if path.extension().is_some_and(|ext| ext == "po") {
            let filename = path.file_stem().expect("catalog filename");
            let module = filename
                .to_str()
                .expect("catalog identifier")
                .replace('-', "_")
                .to_ascii_lowercase();
            assert!(
                module
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "invalid catalog identifier"
            );
            let generated = output.join(filename).with_extension("rs");
            catalogs::compile(&path, &generated)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            modules.push(format!("#[path = {generated:?}]\nmod {module};\n"));
        }
    }
    println!("cargo:rerun-if-changed=tests/fixtures/translation.po");
    catalogs::compile(
        std::path::Path::new("tests/fixtures/translation.po"),
        &output.join("test_translation.rs"),
    )
    .expect("translation regression fixture");
    // Generated files are modules so they retain their own module attributes.
    // Sort directory entries for reproducible builds.
    modules.sort();
    std::fs::write(output.join("catalogs.rs"), modules.concat()).expect("catalog module index");
    std::fs::write(
        output.join("test_catalog.rs"),
        format!(
            "#[path = {:?}]\nmod fixture;\n",
            output.join("test_translation.rs")
        ),
    )
    .expect("test catalog module index");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows/navifast.ico");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("packaging/windows/navifast.ico")
            .set("ProductName", "Navifast")
            .set("FileDescription", "A native Navidrome client");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources not embedded: {error}");
        }
    }
}
