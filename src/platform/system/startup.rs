//! Per-user startup registration. No elevated installation is needed.

pub fn set_enabled(enabled: bool, private: bool) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mode = if private { "--privacy" } else { "--normal" };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let mut command = std::process::Command::new("reg.exe");
        command.creation_flags(0x08000000);
        if enabled {
            command
                .args(["add", key, "/v", "Lennette", "/t", "REG_SZ", "/d"])
                .arg(format!("\"{}\" {mode}", executable.display()))
                .arg("/f");
        } else {
            // Deleting an already absent registration is a successful no-op.
            let exists = std::process::Command::new("reg.exe")
                .creation_flags(0x08000000)
                .args(["query", key, "/v", "Lennette"])
                .output()
                .map_err(|e| e.to_string())?;
            if !exists.status.success() {
                return Ok(());
            }
            command.args(["delete", key, "/v", "Lennette", "/f"]);
        }
        let output = command.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "Windows could not update startup: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    #[cfg(target_os = "linux")]
    {
        let config = directories::BaseDirs::new().ok_or("No user configuration folder")?;
        let directory = config.config_dir().join("autostart");
        let path = directory.join("com.itssingularity.lennette.desktop");
        if enabled {
            std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            let executable = executable
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('`', "\\`")
                .replace('$', "\\$")
                .replace('%', "%%");
            std::fs::write(path, format!("[Desktop Entry]\nType=Application\nName=Lennette\nExec=\"{executable}\" {mode}\nIcon=com.itssingularity.lennette\nTerminal=false\n")).map_err(|e| e.to_string())?;
        } else if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = directories::BaseDirs::new().ok_or("No user home folder")?;
        let directory = home.home_dir().join("Library/LaunchAgents");
        let path = directory.join("com.itssingularity.lennette.plist");
        if enabled {
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let executable = executable
                .to_str()
                .ok_or("The executable path is not valid UTF-8")?;
            std::fs::write(&path, launch_agent(executable, mode)).map_err(|e| e.to_string())?;
        } else {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        // launchd reads this per-user agent at the next login. Do not bootstrap
        // it here: doing so would launch a second instance while toggling settings.
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn launch_agent(executable: &str, mode: &str) -> String {
    let executable = executable
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&apos;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.itssingularity.lennette</string>
<key>ProgramArguments</key><array><string>{executable}</string><string>{mode}</string></array>
<key>RunAtLoad</key><true/>
<key>ProcessType</key><string>Interactive</string>
</dict></plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_agent_preserves_spaces_and_escapes_xml_without_a_shell() {
        let xml = launch_agent(
            "/Users/A & B/Apps/<Lennette>.app/Contents/MacOS/lennette",
            "--privacy",
        );
        assert!(xml.contains("/Users/A &amp; B/Apps/&lt;Lennette&gt;.app/Contents/MacOS/lennette"));
        assert!(xml.contains("<string>--privacy</string>"));
        assert!(!xml.contains("KeepAlive"));
    }
}
