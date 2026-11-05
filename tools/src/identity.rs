//! Source, binary and package identity verification without a Python runtime.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

pub fn run(root: &Path, arguments: &[String]) -> Result<()> {
    let Some(binary) = arguments.first() else {
        bail!(
            "Usage: cargo xtask check-identity <binary> [--write-macos-plist <path>] [--bundle <path>]"
        );
    };
    let metadata = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()?;
    ensure!(metadata.status.success(), "cargo metadata failed");
    let metadata: Value = serde_json::from_slice(&metadata.stdout)?;
    let package = metadata["packages"]
        .as_array()
        .context("missing packages")?
        .iter()
        .find(|package| package["name"] == "lennette")
        .context("missing desktop package")?;
    let mut expected = package["metadata"]["lennette"].clone();
    for (name, source) in [
        ("version", "version"),
        ("description", "description"),
        ("website", "homepage"),
    ] {
        expected[name] = package[source].clone();
    }
    let output = Command::new(Path::new(binary).canonicalize()?)
        .arg("--app-info")
        .output()?;
    ensure!(output.status.success(), "Binary --app-info failed");
    let actual: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        actual == expected,
        "Binary metadata differs from Cargo.toml; rebuild the binary"
    );
    for (key, value) in [
        ("identifier", "com.itssingularity.lennette"),
        ("name", "Lennette"),
        ("publisher", "Singularity Works"),
    ] {
        ensure!(expected[key] == value, "Unexpected application {key}");
    }
    let text = fs::read_to_string(root.join("packaging/com.itssingularity.lennette.desktop"))?;
    let desktop: BTreeMap<_, _> = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    for (entry, key) in [
        ("Name", "name"),
        ("Comment", "description"),
        ("Icon", "identifier"),
        ("StartupWMClass", "identifier"),
        ("X-Lennette-Publisher", "publisher"),
        ("X-Lennette-Website", "website"),
    ] {
        ensure!(
            desktop.get(entry).copied() == expected[key].as_str(),
            "Desktop entry {entry} differs from Cargo.toml"
        );
    }
    let pwa: Value =
        serde_json::from_slice(&fs::read(root.join("apps/web/public/site.webmanifest"))?)?;
    ensure!(
        pwa["name"] == expected["name"] && pwa["description"] == expected["description"],
        "PWA identity differs from Cargo.toml"
    );
    let mut keys = BTreeMap::new();
    for (key, source) in [
        ("CFBundleName", "name"),
        ("CFBundleDisplayName", "name"),
        ("CFBundleIdentifier", "identifier"),
        ("CFBundleShortVersionString", "version"),
        ("CFBundleVersion", "version"),
        ("CFBundleGetInfoString", "description"),
    ] {
        keys.insert(
            key,
            expected[source]
                .as_str()
                .context("identity value must be a string")?
                .to_owned(),
        );
    }
    keys.insert(
        "NSHumanReadableCopyright",
        format!(
            "{} · {}",
            expected["publisher"].as_str().unwrap(),
            expected["website"].as_str().unwrap()
        ),
    );
    let (options, remainder) = arguments[1..].as_chunks::<2>();
    ensure!(remainder.is_empty(), "Missing option value");
    for option in options {
        match option[0].as_str() {
            "--write-macos-plist" => {
                let mut plist = plist::Value::from_file(&option[1])?;
                let dict = plist
                    .as_dictionary_mut()
                    .context("Info.plist must be a dictionary")?;
                for (key, value) in &keys {
                    dict.insert((*key).into(), plist::Value::String(value.clone()));
                }
                plist.to_file_xml(&option[1])?;
            }
            "--bundle" => {
                let bundle = Path::new(&option[1]);
                let plist = plist::Value::from_file(bundle.join("Contents/Info.plist"))?;
                let dict = plist
                    .as_dictionary()
                    .context("Info.plist must be a dictionary")?;
                for (key, value) in &keys {
                    ensure!(
                        dict.get(key).and_then(plist::Value::as_string) == Some(value),
                        "Bundle identity mismatch: {key}"
                    );
                }
                let icon = dict
                    .get("CFBundleIconFile")
                    .and_then(plist::Value::as_string)
                    .context("missing bundle icon")?;
                let resources = bundle.join("Contents/Resources");
                ensure!(
                    fs::read(resources.join(format!("{icon}.icns")))?.starts_with(b"icns"),
                    "Missing or invalid branded icon"
                );
                ensure!(
                    fs::read(resources.join("lennette_app_icon.png"))?
                        == fs::read(root.join("apps/desktop/src/assets/lennette_app_icon.png"))?,
                    "Bundle artwork differs from source"
                );
            }
            flag => bail!("Unknown option: {flag}"),
        }
    }
    println!(
        "Application identity verified: {} {} · {} · {}",
        expected["name"].as_str().unwrap(),
        expected["version"].as_str().unwrap(),
        expected["identifier"].as_str().unwrap(),
        expected["publisher"].as_str().unwrap()
    );
    Ok(())
}
