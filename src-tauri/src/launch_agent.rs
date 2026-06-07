use std::fs;
use std::path::PathBuf;

const LAUNCH_AGENT_LABEL: &str = "com.observer.menubar.launcher";
const LAUNCH_AGENT_FILE: &str = "com.observer.menubar.launcher.plist";

pub fn sync_launch_at_login(enabled: bool) -> Result<bool, String> {
    if enabled {
        install_launch_agent()?;
    } else {
        remove_launch_agent()?;
    }
    Ok(enabled)
}

pub fn launch_at_login_enabled() -> bool {
    launch_agent_path().is_some_and(|path| path.exists())
}

fn install_launch_agent() -> Result<(), String> {
    let app_path = current_app_path()?;
    let plist_path =
        launch_agent_path().ok_or_else(|| "cannot resolve launch agent path".to_string())?;
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let plist = launch_agent_plist(&app_path);
    fs::write(&plist_path, plist).map_err(|error| error.to_string())?;

    let _ = launchctl("bootout", &plist_path);
    launchctl("bootstrap", &plist_path).or_else(|_| launchctl("load", &plist_path))?;
    Ok(())
}

fn remove_launch_agent() -> Result<(), String> {
    let Some(plist_path) = launch_agent_path() else {
        return Ok(());
    };

    let _ = launchctl("bootout", &plist_path);
    let _ = launchctl("unload", &plist_path);
    if plist_path.exists() {
        fs::remove_file(&plist_path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn launchctl(action: &str, plist_path: &PathBuf) -> Result<(), String> {
    let mut command = std::process::Command::new("launchctl");
    match action {
        "bootstrap" | "bootout" => {
            command
                .arg(action)
                .arg(format!("gui/{}", unsafe { libc::getuid() }))
                .arg(plist_path);
        }
        "load" | "unload" => {
            command.arg(action).arg(plist_path);
        }
        _ => return Err(format!("unsupported launchctl action: {action}")),
    }

    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr.trim().to_string())
}

fn current_app_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut current = executable.as_path();
    while let Some(parent) = current.parent() {
        if parent.extension().and_then(|value| value.to_str()) == Some("app") {
            return Ok(parent.to_path_buf());
        }
        current = parent;
    }
    Ok(executable)
}

fn launch_agent_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join("Library")
            .join("LaunchAgents")
            .join(LAUNCH_AGENT_FILE)
    })
}

fn launch_agent_plist(app_path: &PathBuf) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/open</string>
    <string>-a</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        LAUNCH_AGENT_LABEL,
        escape_xml(&app_path.to_string_lossy())
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_escapes_app_path() {
        let plist = launch_agent_plist(&PathBuf::from("/Applications/观察者 & Test.app"));

        assert!(plist.contains("com.observer.menubar.launcher"));
        assert!(plist.contains("/Applications/观察者 &amp; Test.app"));
        assert!(!plist.contains("/Applications/观察者 & Test.app"));
    }
}
