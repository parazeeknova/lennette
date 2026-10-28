#!/usr/bin/env python3
"""Check source/build identity and optionally populate a macOS bundle plist."""
import argparse
import configparser
import json
from pathlib import Path
import plistlib
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def expected_identity():
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--no-deps", "--format-version", "1",
        "--manifest-path", str(ROOT / "Cargo.toml"),
    ], text=True))
    package = next(p for p in metadata["packages"] if p["name"] == "lennette")
    identity = package["metadata"]["lennette"]
    return {**identity, "version": package["version"],
            "description": package["description"], "website": package["homepage"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--write-macos-plist", type=Path)
    parser.add_argument("--bundle", type=Path)
    args = parser.parse_args()
    expected = expected_identity()
    actual = json.loads(subprocess.check_output([str(args.binary.resolve()), "--app-info"], text=True))
    assert actual == expected, f"Binary metadata differs from Cargo.toml; rebuild it: {actual} != {expected}"
    assert expected["identifier"] == "com.itssingularity.lennette"
    assert expected["name"] == "Lennette"
    assert expected["publisher"] == "Singularity Works"
    desktop = configparser.ConfigParser(interpolation=None)
    desktop.read(ROOT / f"packaging/{expected['identifier']}.desktop")
    entry = desktop["Desktop Entry"]
    assert entry["Name"] == expected["name"]
    assert entry["Comment"] == expected["description"]
    assert entry["Icon"] == entry["StartupWMClass"] == expected["identifier"]
    assert entry["X-Lennette-Publisher"] == expected["publisher"]
    assert entry["X-Lennette-Website"] == expected["website"]
    pwa = json.loads((ROOT / "web/public/site.webmanifest").read_text())
    assert pwa["name"] == expected["name"]
    assert pwa["description"] == expected["description"]
    keys = {
        "CFBundleName": expected["name"], "CFBundleDisplayName": expected["name"],
        "CFBundleIdentifier": expected["identifier"],
        "CFBundleShortVersionString": expected["version"], "CFBundleVersion": expected["version"],
        "CFBundleGetInfoString": expected["description"],
        "NSHumanReadableCopyright": f"{expected['publisher']} · {expected['website']}",
    }
    if args.write_macos_plist:
        with args.write_macos_plist.open("rb") as source:
            plist = plistlib.load(source)
        plist.update(keys)
        with args.write_macos_plist.open("wb") as output:
            plistlib.dump(plist, output, sort_keys=False)
    if args.bundle:
        with (args.bundle / "Contents/Info.plist").open("rb") as source:
            plist = plistlib.load(source)
        assert all(plist.get(k) == v for k, v in keys.items()), "Bundle identity mismatch"
        icon = args.bundle / "Contents/Resources" / (plist["CFBundleIconFile"] + ".icns")
        assert icon.read_bytes()[:4] == b"icns", "Missing or invalid branded app icon"
        assert (args.bundle / "Contents/Resources/lennette_app_icon.png").read_bytes() == (
            ROOT / "src/assets/lennette_app_icon.png").read_bytes(), "Bundle artwork differs from source"
    print(f"Application identity verified: {expected['name']} {expected['version']} · {expected['identifier']} · {expected['publisher']}")


if __name__ == "__main__":
    main()
