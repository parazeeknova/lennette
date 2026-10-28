<h1 align="center"><img src="https://swr2.itssingularity.com/lennette/lennette_app_icon.png" width="36" height="36" alt="" /> Lennette</h1>

<p align="center">An ambient real-time meeting and discussion copilot.<br />Live transcripts, useful context, and answers when you need them. Built with Rust + GPUI.</p>


### What it does

- **Listen to both sides.** Microphone and system-audio transcription with separate speaker context.
- **Answer with context.** Streamed suggestions using your prompt, reusable profiles, previous conversations, screenshots, and PDF/text/image attachments.
- **Make it yours.** General and Interview modes, editable answer/follow-up prompts, a cached live model selector, and **No context** when you want a clean start.
- **Choose when it responds.** On questions or after substantive pauses, with Fast / Balanced / Relaxed pacing.
- **Hold to dictate.** Use the input microphone or **Ctrl+Shift+Space**; release inserts a draft for review.
- **Stay out of the way.** Compact overlay, transparent glass theme, global shortcuts, local history, and exports.
- **Keep the input tidy.** The composer grows to five lines, then scrolls. Code and large pastes become square attachments with scrollable previews; sent references can be reopened from the question bubble. Shift+Enter adds a line; Enter sends.
- **Continue where you left off.** Click the pill logo for New, Restore, or Quit. Restore opens saved chats and meeting transcripts; resumed recordings append to the same meeting. Newly recorded meetings also retain their associated AI thread and selected text context. Quit asks for confirmation inside the overlay.
- **Find a setting or model.** Search across all Settings tabs, or filter providers and models from the pill dropdown. Motion follows the Reduce motion preference.

Bring your own keys: **Google Gemini** or **Merge Gateway** for chat; **Deepgram** for speech.

<p align="center">
  <img src="https://swr2.itssingularity.com/lennette/lennette_header.png" alt="Lennette — know exactly what to say, live" width="100%" />
</p>

### Set up once, then run `lenny`

**Linux (x86_64 desktop):**

```sh
curl -fsSL https://lennette.itssingularity.com/install | bash
```

**Windows (x86_64, PowerShell 5.1 or newer):**

```powershell
irm https://lennette.itssingularity.com/install.ps1 | iex
```

Setup downloads and verifies the release, adds a user-local command, and opens
Lennette immediately. No Node.js, Rust, repository clone, or administrator rights.
On Linux, open a new terminal afterward to pick up the PATH change. Windows
PowerShell updates the current PATH too.

From then on, the commands are the same on both platforms:

```sh
lenny                              # launch the installed app
lenny update                       # install the latest release
lenny update --version <tag>        # select a published release
lenny doctor                       # check the executable and desktop environment
lenny uninstall                    # remove releases, keeping settings and keys
```

Launches use the local binary and work offline. Missing payloads are repaired on
the next launch; updates require a connection. Updates verify and check the new
executable before switching the launcher, keeping old versions until uninstall.
For setup without opening the app, use `bash -s -- --no-launch` on Linux or
`& ([scriptblock]::Create((irm https://lennette.itssingularity.com/install.ps1))) -NoLaunch`
on Windows.

The two native setup scripts live directly in `web/public/install` and
`web/public/install.ps1`, served as `/install` and `/install.ps1`. There are no
root copies or generated copies. Bash and PowerShell need different setup
syntax; after setup, users only need `lenny`. Windows exposes one `lenny.cmd`
command, avoiding a conflicting `lenny.ps1` that execution policy could block.

Linux needs the release build's glibc version and your distribution's desktop,
Vulkan, and audio libraries; these are not bundled in the archive. Setup reports
loader errors, and `lenny doctor` checks missing libraries and a desktop session.
On NixOS, use the Nix installation below to supply the interpreter and runtime
libraries. macOS app DMGs and ZIPs are available for Apple Silicon and Intel; see the macOS section below. Linux and Windows ARM builds are not available.

Prefer a plain download? Grab a build from [Releases](https://github.com/parazeeknova/lennette/releases). SHA-256 checksums are included.

Windows also has a **setup wizard** (`lennette-windows-x86_64-setup.exe`) with a
Start menu shortcut, optional desktop shortcut, and an uninstaller. It installs
for the current user without administrator rights and keeps settings and keys
when upgraded or removed. This is an alternative to the `lenny` command setup.

### Screen sharing

**Windows:** the overlay and dashboard use OS capture exclusion, including for full-monitor sharing on capture paths that honor it. They stay visible locally. Private launch is the default, with no tray icon, taskbar entry, or Alt+Tab entry. The new Windows backend still needs real meeting/audio validation; check the receiver's view in your meeting software. Native dialogs, notifications, and the tray are outside window capture protection.

**macOS:** legacy window capture protection is best effort. Full-display sharing can include Lennette even in Privacy mode. Share a specific application window and verify the receiver's view. Modern ScreenCaptureKit clients control their own exclusion filters; Lennette cannot force every recorder to omit its windows.

**Linux:** capture exclusion requires the **patched Hyprland compositor** below. Ordinary X11/Wayland launches do not provide it. [Protection boundary](https://github.com/parazeeknova/lennette/blob/main/packaging/hyprland/README.md#protection-boundary).


### Linux / NixOS

With [Nix](https://nixos.org/download/) and [devenv](https://devenv.sh/getting-started/) installed:

```sh
git clone https://github.com/parazeeknova/lennette.git
cd lennette
devenv shell -- cargo run --locked
# Install the optimized binary, icon, and desktop entry locally:
devenv shell -- just install
```

The shell supplies native libraries; installation preserves their Nix runtime paths. Keep `~/.local/bin` on your `PATH`. Without Nix, install the packages in [release.yml](https://github.com/parazeeknova/lennette/blob/main/.github/workflows/release.yml) and use `cargo run --locked`.

### macOS

macOS 13+ on Apple Silicon and Intel is supported. Download the matching app DMG or ZIP
from [Releases](https://github.com/parazeeknova/lennette/releases), open it, and
drag `Lennette.app` to Applications. Builds are ad-hoc signed, not notarized.
Allow Microphone and Screen & System Audio Recording in System Settings.

From source, install Apple's Command Line Tools and Rust, then run
`cargo run --locked`. To package an app, run `cargo build --locked --release`
and `bash scripts/build-macos-app.sh`. Full Xcode is not required. The macOS
GPUI dependency explicitly enables `font-kit`; without it GPUI uses a placeholder
text backend and launches with blank text. `tests/macos_text.rs` guards this.
The branded app includes a multi-resolution Finder/Dock icon. Its bundle identity
is `com.itssingularity.lennette`. The release also includes a branded drag-to-Applications
DMG; the ZIP is available as an alternative.

System audio uses ScreenCaptureKit (no virtual driver) and captures the system mix,
excluding Lennette's own output. The selected output device does not filter that mix.
After changing recording permissions, stop/restart listening and relaunch if macOS asks.
Screenshots use ScreenCaptureKit with explicit self-exclusion on macOS 14+; Ventura
retains the legacy screenshot path. This affects screenshots taken by Lennette only.

The Control+Shift defaults below work while other apps are focused. Carbon hotkeys
need no Accessibility/Input Monitoring permission; conflicts are logged. Keycodes
refer to physical ANSI positions. Command-Q opens the quit confirmation; reopening
from Finder or the Dock restores the overlay. Privacy mode hides Dock/Command-Tab
and the menu-bar item. Always-on-top uses the native floating window level and both
windows can appear on full-screen Spaces. Native traffic-light controls are hidden
on both windows; Lennette keeps its own controls and native dragging/resizing.

Launch on startup registers
`~/Library/LaunchAgents/com.itssingularity.lennette.plist` for the next login.
Keep the app at its installed location. Disabling startup removes that registration;
uninstalling the app keeps settings and keys.

Local verification: `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace --all-targets`, and
`bash scripts/macos-smoke.sh target/debug/lennette`. The native check exercises
window policy, Spaces, stacking, Dock/menu-bar lifecycle, hotkey registration,
conflict detection, dispatch/cleanup, and real CoreMedia PCM buffering and errors.
It requests no recording permissions. Before release, manually test real microphone
and call audio, permission denial/revocation, Retina/external displays, next-login
startup, and shortcuts while hidden. Validate each meeting app from a separate
receiver; full-display invisibility is not guaranteed by these checks.
Developer ID signing and notarization require the maintainer's credentials.

### Windows

Install [Rust](https://rustup.rs/), Visual Studio Build Tools' **Desktop development with C++** workload, and a Windows SDK. In Developer PowerShell:

```powershell
git clone https://github.com/parazeeknova/lennette.git
cd lennette
cargo run --locked
# Portable release build:
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --locked --release --target x86_64-pc-windows-msvc
.\target\x86_64-pc-windows-msvc\release\lennette.exe
```

### Hyprland / your own config

The optional `private_overlay` patch excludes protected windows and their popups from supported capture paths. It is a pinned compositor build, not a loadable plugin.

Add this input to your system flake:

```nix
inputs.lennette-private.url = "github:parazeeknova/lennette?dir=packaging/hyprland";
```

With integrated Home Manager, add to your NixOS configuration:

```nix
{ inputs, ... }: {
  imports = [ inputs.lennette-private.nixosModules.default ];
  programs.lennettePrivateOverlay.enable = true;
  home-manager.users.YOUR_USER = {
    imports = [ inputs.lennette-private.homeManagerModules.default ];
    programs.lennettePrivateOverlay.enable = true;
    # Optional: programs.lennettePrivateOverlay.executable = "/absolute/path/to/lennette";
    wayland.windowManager.hyprland.package = null;
  };
}
```

Install Lennette separately, update your flake lock, rebuild, then **log out and back in**. Run `lennette-private-user --check`, then `lennette-private-user`. The module adds the protected launcher and bindings to your Lua config. Keep this input's pinned `nixpkgs`; plugins must be rebuilt for its compositor and explicitly allowed. [Full integration guide](https://github.com/parazeeknova/lennette/blob/main/packaging/hyprland/README.md).

<p align="center">
  <img src="https://swr2.itssingularity.com/lennette/lennette.png" alt="Lennette himself" width="100" />
  <br />
  <sub>Know exactly what to say, live.</sub>
</p>

### Shortcuts

| Keys | Action |
| --- | --- |
| Ctrl+Shift+Backslash | Show / hide |
| Ctrl+Shift+D | Dashboard |
| Ctrl+Shift+I | Focus input |
| Ctrl+Shift+A | Start / stop listening |
| Ctrl+Shift+M | Toggle system audio |
| Ctrl+Shift+S | Attach screenshot |
| Ctrl+Shift+Space | Hold to dictate with input focused |

While hidden, only **Show / hide** works; active meeting capture continues. Wayland hotkeys need a supported desktop portal or compositor bindings. The Hyprland module configures protected bindings using the physical backslash keycode.

### Shared application identity

Lennette is created by **Singularity Works** · [itssingularity.com](https://itssingularity.com).
`Cargo.toml` is the source for the version, description, display name, publisher,
and `com.itssingularity.lennette` application ID. All three desktop platforms use
the same Cargo package version. Packaging reads `lennette --app-info` from the actual
binary, including Windows version resources and the Mac bundle. The Rust crate
and CLI executable remain `lennette`. `cargo xtask bump` updates the manifest and
lockfile together. Existing data moves to the new platform data location as one
directory, keeping the encrypted vault and key together; it is never overwritten.

### Your data

History, settings, profiles, and the AES-256-GCM credential vault live locally in your user data directory. The vault key is a local file. Audio goes to your speech provider; generation sends your prompt, selected context/attachments, bounded chat history, and both sides of the live transcript to your chat provider. Suggestions stay in the app; Lennette does not speak or send them into your call.

<p align="center">
  <img src="https://swr2.itssingularity.com/lennette/lennette_footer.png" alt="Lennette" width="100%" />
</p>
