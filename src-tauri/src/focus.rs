use std::process::Stdio;
use std::time::{Duration, Instant};

const OSASCRIPT_TIMEOUT: Duration = Duration::from_millis(2_500);

pub fn focus_agent_window(
    agent_type: &str,
    cwd: &str,
    project_name: &str,
    pid: Option<u32>,
) -> Result<String, String> {
    let tty = pid.and_then(process_tty);

    if let Some(tty) = tty.as_deref() {
        if run_focus_script(
            "Terminal",
            &terminal_focus_script(cwd, project_name, Some(tty)),
        )
        .is_ok()
        {
            return Ok("已按 TTY 定位 Terminal 会话".to_string());
        }

        if run_focus_script("iTerm", &iterm_focus_script(cwd, project_name, Some(tty))).is_ok() {
            return Ok("已按 TTY 定位 iTerm 会话".to_string());
        }
    }

    if agent_type == "Codex" && focus_application("Codex").is_ok() {
        return Ok("已聚焦 Codex".to_string());
    }

    if !cwd.trim().is_empty() {
        if run_focus_script("Terminal", &terminal_focus_script(cwd, project_name, None)).is_ok() {
            return Ok("已尝试定位 Terminal 会话".to_string());
        }

        if run_focus_script("iTerm", &iterm_focus_script(cwd, project_name, None)).is_ok() {
            return Ok("已尝试定位 iTerm 会话".to_string());
        }
    }

    let fallback = if agent_type == "Codex" {
        "Codex"
    } else {
        "Terminal"
    };
    focus_application(fallback)?;
    Ok(format!("已聚焦 {fallback}"))
}

fn focus_application(app_name: &str) -> Result<(), String> {
    run_osascript(&format!(
        r#"tell application "{}" to activate"#,
        escape_applescript_string(app_name)
    ))
}

fn process_tty(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_tty(&String::from_utf8_lossy(&output.stdout))
}

fn normalize_tty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "??" || trimmed == "?" {
        return None;
    }
    Some(trimmed.trim_start_matches("/dev/").to_string())
}

fn run_focus_script(app_name: &str, script: &str) -> Result<(), String> {
    if !app_running(app_name) {
        return Err(format!("{app_name} is not running"));
    }
    run_osascript(script)
}

fn app_running(app_name: &str) -> bool {
    if app_name == "iTerm" {
        return ["iTerm2", "iTerm"]
            .iter()
            .any(|candidate| process_running(candidate));
    }
    process_running(app_name)
}

fn process_running(process_name: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", process_name])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn terminal_focus_script(cwd: &str, project_name: &str, tty: Option<&str>) -> String {
    let cwd = escape_applescript_string(cwd);
    let project_name = escape_applescript_string(project_name);
    let tty = escape_applescript_string(tty.unwrap_or_default());
    format!(
        r#"tell application "Terminal"
  set targetPath to "{}"
  set targetName to "{}"
  set targetTty to "{}"
  repeat with w in windows
    repeat with t in tabs of w
      try
        set tabTty to tty of t
      on error
        set tabTty to ""
      end try
      if targetTty is not "" and tabTty contains targetTty then
        set selected tab of w to t
        set index of w to 1
        activate
        return
      end if
      try
        set tabTitle to custom title of t
      on error
        set tabTitle to ""
      end try
      set tabText to contents of t
      if tabText contains targetPath or tabTitle contains targetPath or (targetName is not "" and (tabText contains targetName or tabTitle contains targetName)) then
        set selected tab of w to t
        set index of w to 1
        activate
        return
      end if
    end repeat
  end repeat
end tell
error "no matching Terminal tab""#,
        cwd, project_name, tty
    )
}

fn iterm_focus_script(cwd: &str, project_name: &str, tty: Option<&str>) -> String {
    let cwd = escape_applescript_string(cwd);
    let project_name = escape_applescript_string(project_name);
    let tty = escape_applescript_string(tty.unwrap_or_default());
    format!(
        r#"tell application "iTerm"
  set targetPath to "{}"
  set targetName to "{}"
  set targetTty to "{}"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        try
          set sessionTty to tty of s
        on error
          set sessionTty to ""
        end try
        if targetTty is not "" and sessionTty contains targetTty then
          select t
          select w
          activate
          return
        end if
        try
          set sessionName to name of s
        on error
          set sessionName to ""
        end try
        set sessionText to contents of s
        if sessionText contains targetPath or sessionName contains targetPath or (targetName is not "" and (sessionText contains targetName or sessionName contains targetName)) then
          select t
          select w
          activate
          return
        end if
      end repeat
    end repeat
  end repeat
end tell
error "no matching iTerm session""#,
        cwd, project_name, tty
    )
}

fn run_osascript(script: &str) -> Result<(), String> {
    run_osascript_with_output(script).map(|_| ())
}

fn run_osascript_with_output(script: &str) -> Result<String, String> {
    let mut child = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;

    let started_at = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        if started_at.elapsed() >= OSASCRIPT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("osascript timed out while focusing agent".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_script_contains_path_and_project() {
        let script = terminal_focus_script("/Users/test/my app", "my app", None);

        assert!(script.contains("/Users/test/my app"));
        assert!(script.contains("my app"));
        assert!(script.contains("selected tab"));
    }

    #[test]
    fn terminal_script_prefers_tty_when_present() {
        let script = terminal_focus_script("/Users/test/project", "project", Some("ttys003"));

        assert!(script.contains("set targetTty to \"ttys003\""));
        assert!(script.contains("tabTty contains targetTty"));
    }

    #[test]
    fn normalizes_ps_tty_output() {
        assert_eq!(normalize_tty("ttys003\n").as_deref(), Some("ttys003"));
        assert_eq!(normalize_tty("/dev/ttys004").as_deref(), Some("ttys004"));
        assert_eq!(normalize_tty("??"), None);
    }

    #[test]
    fn escapes_applescript_strings() {
        assert_eq!(
            escape_applescript_string(r#"/tmp/a"b\c"#),
            r#"/tmp/a\"b\\c"#
        );
    }
}
