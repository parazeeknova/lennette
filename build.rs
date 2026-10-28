fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=packaging/windows/lennette.rc");
    let manifest: toml_edit::DocumentMut = std::fs::read_to_string("Cargo.toml")
        .expect("read app manifest")
        .parse()
        .expect("parse app manifest");
    let metadata = &manifest["package"]["metadata"]["lennette"];
    let identifier = metadata["identifier"].as_str().expect("app identifier");
    let name = metadata["name"].as_str().expect("app name");
    let publisher = metadata["publisher"].as_str().expect("app publisher");
    let version = std::env::var("CARGO_PKG_VERSION").expect("app version");
    let description = std::env::var("CARGO_PKG_DESCRIPTION").expect("app description");
    let website = std::env::var("CARGO_PKG_HOMEPAGE").expect("publisher website");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("build output"));
    let constants = [
        ("APP_ID", identifier),
        ("NAME", name),
        ("PUBLISHER", publisher),
        ("VERSION", &version),
        ("DESCRIPTION", &description),
        ("WEBSITE", &website),
    ]
    .into_iter()
    .map(|(key, value)| format!("pub const {key}: &str = {value:?};\n"))
    .collect::<String>();
    std::fs::write(output.join("identity.rs"), constants).expect("write app identity");
    println!("cargo:rerun-if-changed=src/assets/lennette.ico");
    println!("cargo:rerun-if-changed=src/platform/macos/native.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/platform/macos/native.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .flag("-mmacosx-version-min=13.0")
            .compile("lennette_macos");
        for framework in [
            "AppKit",
            "Carbon",
            "ScreenCaptureKit",
            "CoreMedia",
            "CoreAudio",
            "CoreGraphics",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::env::current_dir()
            .expect("repository root")
            .join("src/assets/lennette.ico")
            .to_string_lossy()
            .replace('\\', "/");
        let resources = std::fs::read_to_string("packaging/windows/lennette.rc")
            .expect("read Windows resources")
            .replace("@ICON_PATH@", &icon)
            .replace(
                "@VERSION_COMMAS@",
                &format!("{},0", version.replace('.', ",")),
            )
            .replace("@VERSION@", &version)
            .replace("@APP_ID@", identifier)
            .replace("@APP_NAME@", name)
            .replace("@PUBLISHER@", publisher)
            .replace("@DESCRIPTION@", &description);
        let resource_path = output.join("lennette.rc");
        std::fs::write(&resource_path, resources).expect("write Windows resources");
        embed_resource::compile(resource_path, embed_resource::NONE)
            .manifest_required()
            .expect("embed the Lennette application icon");
    }
}
