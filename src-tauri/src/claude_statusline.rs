use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

const STATUSLINE_SCRIPT: &str = r#"#!/bin/bash
# Observer StatusLine hook: writes Claude rate-limit data for Observer to read.
INPUT=""
while IFS= read -r -t 5 line || [ -n "$line" ]; do
    INPUT="${INPUT}${line}
"
done
[ -z "$INPUT" ] && exit 0
printf '%s' "$INPUT" | python3 -c "
import sys, json, time, os
data = json.load(sys.stdin)
rl = data.get('rate_limits')
if not rl:
    sys.exit(0)
out = {'source': 'claude', 'updated_at': int(time.time())}
fh = rl.get('five_hour')
if fh:
    out['five_hour'] = {'used_percentage': fh.get('used_percentage', 0), 'resets_at': fh.get('resets_at', 0)}
sd = rl.get('seven_day')
if sd:
    out['seven_day'] = {'used_percentage': sd.get('used_percentage', 0), 'resets_at': sd.get('resets_at', 0)}
config_dir = os.environ.get('CLAUDE_CONFIG_DIR', os.path.join(os.path.expanduser('~'), '.claude'))
with open(os.path.join(config_dir, 'abtop-rate-limits.json'), 'w') as f:
    json.dump(out, f)
" 2>/dev/null
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeStatusLineStatus {
    pub config_dir: String,
    pub settings_path: String,
    pub script_path: String,
    pub rate_file_path: String,
    pub script_exists: bool,
    pub rate_file_exists: bool,
    pub configured_command: Option<String>,
    pub installed: bool,
    pub conflict: bool,
}

pub fn status() -> ClaudeStatusLineStatus {
    let config_dir = claude_dir();
    let settings_path = config_dir.join("settings.json");
    let script_path = config_dir.join("observer-statusline.sh");
    let abtop_script_path = config_dir.join("abtop-statusline.sh");
    let rate_file_path = config_dir.join("abtop-rate-limits.json");
    let configured_command = configured_statusline_command(&settings_path);
    let expected_commands = [
        script_path.display().to_string(),
        abtop_script_path.display().to_string(),
    ];
    let installed = configured_command
        .as_ref()
        .is_some_and(|command| expected_commands.iter().any(|expected| expected == command));
    let conflict = configured_command
        .as_ref()
        .is_some_and(|command| !command.is_empty() && !installed);

    ClaudeStatusLineStatus {
        config_dir: config_dir.display().to_string(),
        settings_path: settings_path.display().to_string(),
        script_path: script_path.display().to_string(),
        rate_file_path: rate_file_path.display().to_string(),
        script_exists: script_path.exists() || abtop_script_path.exists(),
        rate_file_exists: rate_file_path.exists(),
        configured_command,
        installed,
        conflict,
    }
}

pub fn install() -> Result<ClaudeStatusLineStatus, String> {
    let config_dir = claude_dir();
    std::fs::create_dir_all(&config_dir).map_err(|error| error.to_string())?;
    let script_path = config_dir.join("observer-statusline.sh");
    let settings_path = config_dir.join("settings.json");

    let current = status();
    if current.conflict {
        return Err(format!(
            "Claude statusLine 已配置为其他命令：{}",
            current.configured_command.unwrap_or_default()
        ));
    }

    std::fs::write(&script_path, STATUSLINE_SCRIPT).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }

    let mut settings = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path).map_err(|error| error.to_string())?;
        serde_json::from_str::<Value>(&raw).map_err(|error| error.to_string())?
    } else {
        Value::Object(Default::default())
    };

    if !settings.is_object() {
        settings = Value::Object(Default::default());
    }
    let obj = settings.as_object_mut().expect("settings is object");
    obj.insert(
        "statusLine".to_string(),
        serde_json::json!({
            "type": "command",
            "command": script_path.display().to_string()
        }),
    );

    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    Ok(status())
}

fn claude_dir() -> PathBuf {
    std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

fn configured_statusline_command(settings_path: &PathBuf) -> Option<String> {
    let raw = std::fs::read_to_string(settings_path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value
        .get("statusLine")
        .and_then(|statusline| statusline.get("command"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_with_frontend_names() {
        let status = ClaudeStatusLineStatus {
            config_dir: "/tmp/.claude".to_string(),
            settings_path: "/tmp/.claude/settings.json".to_string(),
            script_path: "/tmp/.claude/observer-statusline.sh".to_string(),
            rate_file_path: "/tmp/.claude/abtop-rate-limits.json".to_string(),
            script_exists: true,
            rate_file_exists: false,
            configured_command: None,
            installed: false,
            conflict: false,
        };

        let raw = serde_json::to_string(&status).unwrap();

        assert!(raw.contains("configDir"));
        assert!(raw.contains("scriptExists"));
        assert!(!raw.contains("config_dir"));
    }
}
