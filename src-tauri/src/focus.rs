use std::process::Stdio;
use std::time::{Duration, Instant};

const OSASCRIPT_TIMEOUT: Duration = Duration::from_millis(2_500);

pub fn focus_agent_window(
    agent_type: &str,
    cwd: &str,
    project_name: &str,
    pid: Option<u32>,
    child_pids: Option<Vec<u32>>,
) -> Result<String, String> {
    let target = FocusTarget::new(cwd, project_name, tty_candidates(pid, child_pids));

    for tty in &target.ttys {
        if run_focus_script("Terminal", &terminal_focus_script(&target, Some(tty))).is_ok() {
            return Ok("已按 TTY 定位 Terminal 会话".to_string());
        }

        if run_focus_script("iTerm", &iterm_focus_script(&target, Some(tty))).is_ok() {
            return Ok("已按 TTY 定位 iTerm 会话".to_string());
        }
    }

    if agent_type == "Codex" && focus_application("Codex").is_ok() {
        return Ok("已聚焦 Codex".to_string());
    }

    if !target.terms.is_empty() {
        if run_focus_script("Terminal", &terminal_focus_script(&target, None)).is_ok() {
            return Ok("已尝试定位 Terminal 会话".to_string());
        }

        if run_focus_script("iTerm", &iterm_focus_script(&target, None)).is_ok() {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FocusTarget {
    terms: Vec<String>,
    ttys: Vec<String>,
}

impl FocusTarget {
    fn new(cwd: &str, project_name: &str, ttys: Vec<String>) -> Self {
        Self {
            terms: focus_terms(cwd, project_name),
            ttys,
        }
    }
}

fn tty_candidates(pid: Option<u32>, child_pids: Option<Vec<u32>>) -> Vec<String> {
    let mut candidates = Vec::new();
    for pid in pid.into_iter().chain(child_pids.unwrap_or_default()) {
        if let Some(tty) = process_tty(pid) {
            push_unique(&mut candidates, tty);
        }
    }
    candidates
}

fn focus_terms(cwd: &str, project_name: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let cwd = cwd.trim();
    let project_name = project_name.trim();

    if !cwd.is_empty() {
        push_unique(&mut terms, cwd.to_string());
        if let Some(short) = home_short_path(cwd) {
            push_unique(&mut terms, short);
        }
        for suffix in path_suffixes(cwd) {
            push_unique(&mut terms, suffix);
        }
    }

    if !project_name.is_empty() && !is_temporary_chat_name(project_name) {
        push_unique(&mut terms, project_name.to_string());
    }

    terms
}

fn path_suffixes(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .collect();
    let mut suffixes = Vec::new();
    for len in [3_usize, 2, 1] {
        if parts.len() >= len {
            suffixes.push(parts[parts.len() - len..].join("/"));
        }
    }
    suffixes
}

fn home_short_path(path: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    path.strip_prefix(&home)
        .map(|rest| format!("~{}", rest))
        .filter(|value| value != "~")
}

fn is_temporary_chat_name(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("临时对话")
        || lower.contains("temporary")
        || lower.contains("files-mentioned-by-the-user")
        || lower.contains("uploaded-files")
}

fn push_unique(values: &mut Vec<String>, value: String) {
    let value = value.trim().to_string();
    if value.is_empty() || values.iter().any(|existing| existing == &value) {
        return;
    }
    values.push(value);
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

fn terminal_focus_script(target: &FocusTarget, tty: Option<&str>) -> String {
    let terms = applescript_list(&target.terms);
    let tty = escape_applescript_string(tty.unwrap_or_default());
    format!(
        r#"tell application "Terminal"
  set targetTerms to {{{}}}
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
      repeat with targetTerm in targetTerms
        if targetTerm is not "" and (tabText contains targetTerm or tabTitle contains targetTerm) then
          set selected tab of w to t
          set index of w to 1
          activate
          return
        end if
      end repeat
    end repeat
  end repeat
end tell
error "no matching Terminal tab""#,
        terms, tty
    )
}

fn iterm_focus_script(target: &FocusTarget, tty: Option<&str>) -> String {
    let terms = applescript_list(&target.terms);
    let tty = escape_applescript_string(tty.unwrap_or_default());
    format!(
        r#"tell application "iTerm"
  set targetTerms to {{{}}}
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
        repeat with targetTerm in targetTerms
          if targetTerm is not "" and (sessionText contains targetTerm or sessionName contains targetTerm) then
            select t
            select w
            activate
            return
          end if
        end repeat
      end repeat
    end repeat
  end repeat
end tell
error "no matching iTerm session""#,
        terms, tty
    )
}

fn applescript_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("\"{}\"", escape_applescript_string(value)))
        .collect::<Vec<_>>()
        .join(", ")
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
        let target = FocusTarget::new("/Users/test/my app", "my app", vec![]);
        let script = terminal_focus_script(&target, None);

        assert!(script.contains("/Users/test/my app"));
        assert!(script.contains("my app"));
        assert!(script.contains("test/my app"));
        assert!(script.contains("targetTerms"));
        assert!(script.contains("selected tab"));
    }

    #[test]
    fn terminal_script_prefers_tty_when_present() {
        let target = FocusTarget::new("/Users/test/project", "project", vec![]);
        let script = terminal_focus_script(&target, Some("ttys003"));

        assert!(script.contains("set targetTty to \"ttys003\""));
        assert!(script.contains("tabTty contains targetTty"));
    }

    #[test]
    fn focus_terms_include_path_suffixes_and_project() {
        let terms = focus_terms("/Users/test/Workspace/Observer App", "Observer App");

        assert_eq!(
            terms,
            vec![
                "/Users/test/Workspace/Observer App",
                "test/Workspace/Observer App",
                "Workspace/Observer App",
                "Observer App"
            ]
        );
    }

    #[test]
    fn focus_terms_skip_temporary_project_names() {
        let terms = focus_terms("/Users/test/Documents/Codex/tmp-chat", "Codex 临时对话");

        assert!(terms.contains(&"/Users/test/Documents/Codex/tmp-chat".to_string()));
        assert!(!terms.contains(&"Codex 临时对话".to_string()));
    }

    #[test]
    fn applescript_list_escapes_terms() {
        assert_eq!(
            applescript_list(&[
                "/tmp/a".to_string(),
                r#"/tmp/b"c"#.to_string(),
                r#"/tmp/d\e"#.to_string()
            ]),
            r#""/tmp/a", "/tmp/b\"c", "/tmp/d\\e""#
        );
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
