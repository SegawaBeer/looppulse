use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SETTINGS_DIR: &str = "observer";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_plan")]
    pub plan: String,
    #[serde(default)]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default = "default_true")]
    pub notify_critical: bool,
    #[serde(default = "default_true")]
    pub notify_warning: bool,
    #[serde(default = "default_true")]
    pub notify_completion: bool,
    #[serde(default)]
    pub notify_pro_hints: bool,
    #[serde(default = "default_cooldown_minutes")]
    pub cooldown_minutes: u64,
    #[serde(default = "default_refresh_interval_seconds")]
    pub refresh_interval_seconds: u64,
    #[serde(default = "default_enabled_agents")]
    pub enabled_agents: Vec<String>,
    #[serde(default)]
    pub hidden_projects: Vec<String>,
    #[serde(default)]
    pub claude_data_roots: Vec<String>,
    #[serde(default)]
    pub codex_data_roots: Vec<String>,
    #[serde(default)]
    pub opencode_data_roots: Vec<String>,
    #[serde(default = "default_path_display_mode")]
    pub path_display_mode: String,
    #[serde(default = "default_remote_preview_fields")]
    pub remote_preview_fields: Vec<String>,
    #[serde(default = "default_context_warning_percent")]
    pub context_warning_percent: f64,
    #[serde(default = "default_context_critical_percent")]
    pub context_critical_percent: f64,
    #[serde(default = "default_stalled_warning_minutes")]
    pub stalled_warning_minutes: u64,
    #[serde(default = "default_stalled_critical_minutes")]
    pub stalled_critical_minutes: u64,
    #[serde(default = "default_token_warning_threshold")]
    pub token_warning_threshold: u64,
    #[serde(default = "default_true")]
    pub history_enabled: bool,
    #[serde(default = "default_history_retention_days")]
    pub history_retention_days: u64,
    #[serde(default)]
    pub onboarding_completed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            plan: default_plan(),
            notifications_enabled: false,
            launch_at_login: false,
            notify_critical: true,
            notify_warning: true,
            notify_completion: true,
            notify_pro_hints: false,
            cooldown_minutes: default_cooldown_minutes(),
            refresh_interval_seconds: default_refresh_interval_seconds(),
            enabled_agents: default_enabled_agents(),
            hidden_projects: vec![],
            claude_data_roots: vec![],
            codex_data_roots: vec![],
            opencode_data_roots: vec![],
            path_display_mode: default_path_display_mode(),
            remote_preview_fields: default_remote_preview_fields(),
            context_warning_percent: default_context_warning_percent(),
            context_critical_percent: default_context_critical_percent(),
            stalled_warning_minutes: default_stalled_warning_minutes(),
            stalled_critical_minutes: default_stalled_critical_minutes(),
            token_warning_threshold: default_token_warning_threshold(),
            history_enabled: true,
            history_retention_days: default_history_retention_days(),
            onboarding_completed: false,
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.plan = normalize_plan(&self.plan);
        self.cooldown_minutes = self.cooldown_minutes.clamp(1, 120);
        self.refresh_interval_seconds = self.refresh_interval_seconds.clamp(2, 60);
        self.context_warning_percent = self.context_warning_percent.clamp(50.0, 98.0);
        self.context_critical_percent = self
            .context_critical_percent
            .clamp(self.context_warning_percent + 1.0, 100.0);
        self.stalled_warning_minutes = self.stalled_warning_minutes.clamp(3, 120);
        self.stalled_critical_minutes = self
            .stalled_critical_minutes
            .clamp(self.stalled_warning_minutes + 1, 240);
        self.token_warning_threshold = self.token_warning_threshold.clamp(10_000, 50_000_000);
        self.history_retention_days = self.history_retention_days.clamp(1, 365);
        self.path_display_mode = normalize_path_display_mode(&self.path_display_mode);

        self.enabled_agents = dedupe_non_empty(self.enabled_agents);
        self.hidden_projects = dedupe_non_empty(self.hidden_projects);
        self.claude_data_roots = dedupe_non_empty(self.claude_data_roots);
        self.codex_data_roots = dedupe_non_empty(self.codex_data_roots);
        self.opencode_data_roots = dedupe_non_empty(self.opencode_data_roots);
        self.remote_preview_fields = normalize_remote_preview_fields(self.remote_preview_fields);
        self
    }

    pub fn agent_enabled(&self, agent_name: &str) -> bool {
        self.enabled_agents
            .iter()
            .any(|enabled| enabled.eq_ignore_ascii_case(agent_name))
    }

    pub fn hides_session(&self, project_name: &str, cwd: &str) -> bool {
        let project_lower = project_name.to_ascii_lowercase();
        let cwd_lower = cwd.to_ascii_lowercase();
        self.hidden_projects.iter().any(|rule| {
            let rule = rule.to_ascii_lowercase();
            !rule.is_empty() && (project_lower.contains(&rule) || cwd_lower.contains(&rule))
        })
    }

    pub fn is_pro(&self) -> bool {
        self.plan == "pro"
    }

    pub fn claude_data_roots(&self) -> Vec<PathBuf> {
        data_roots(&self.claude_data_roots, ".claude")
    }

    pub fn codex_data_roots(&self) -> Vec<PathBuf> {
        data_roots(&self.codex_data_roots, ".codex")
    }

    pub fn opencode_data_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".local/share/opencode"));
        }
        if let Some(data_dir) = dirs::data_local_dir().or_else(dirs::data_dir) {
            let opencode_dir = data_dir.join("opencode");
            if !roots.iter().any(|path| path == &opencode_dir) {
                roots.push(opencode_dir);
            }
        }

        for root in &self.opencode_data_roots {
            roots.push(expand_user_path(root));
        }

        dedupe_paths(roots)
    }
}

pub fn load_settings() -> AppSettings {
    let Ok(path) = settings_path() else {
        return AppSettings::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return AppSettings::default();
    };
    serde_json::from_str::<AppSettings>(&raw)
        .map(AppSettings::normalized)
        .unwrap_or_default()
}

pub fn save_settings(settings: AppSettings) -> Result<AppSettings, String> {
    let settings = settings.normalized();
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let raw = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    fs::write(path, raw).map_err(|error| error.to_string())?;
    Ok(settings)
}

fn settings_path() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join(SETTINGS_DIR).join(SETTINGS_FILE))
        .ok_or_else(|| "cannot resolve user config directory".to_string())
}

fn dedupe_non_empty(values: Vec<String>) -> Vec<String> {
    let mut next = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !next
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            next.push(trimmed.to_string());
        }
    }
    next
}

fn normalize_plan(plan: &str) -> String {
    if plan.eq_ignore_ascii_case("pro") {
        "pro".to_string()
    } else {
        "free".to_string()
    }
}

fn normalize_path_display_mode(mode: &str) -> String {
    match mode {
        "private" | "compact" | "full" => mode.to_string(),
        _ => default_path_display_mode(),
    }
}

fn normalize_remote_preview_fields(fields: Vec<String>) -> Vec<String> {
    let allowed = allowed_remote_preview_fields();
    let mut next = Vec::new();
    for field in fields {
        let field = field.trim();
        if allowed.iter().any(|allowed| allowed == field)
            && !next.iter().any(|existing: &String| existing == field)
        {
            next.push(field.to_string());
        }
    }
    if next.is_empty() {
        default_remote_preview_fields()
    } else {
        next
    }
}

fn data_roots(custom_roots: &[String], default_home_child: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(default_home_child));
    }

    for root in custom_roots {
        roots.push(expand_user_path(root));
    }

    dedupe_paths(roots)
}

fn expand_user_path(value: &str) -> PathBuf {
    let trimmed = value.trim();
    if trimmed == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(trimmed));
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut next = Vec::new();
    for path in paths {
        let display = path.to_string_lossy().to_string();
        if !next
            .iter()
            .any(|existing: &PathBuf| existing.to_string_lossy() == display)
        {
            next.push(path);
        }
    }
    next
}

fn default_plan() -> String {
    "free".to_string()
}

fn default_path_display_mode() -> String {
    "compact".to_string()
}

fn allowed_remote_preview_fields() -> Vec<String> {
    vec![
        "identity".to_string(),
        "status".to_string(),
        "risk".to_string(),
        "tokens".to_string(),
        "context".to_string(),
        "path".to_string(),
        "environment".to_string(),
        "timeline".to_string(),
    ]
}

fn default_remote_preview_fields() -> Vec<String> {
    vec![
        "identity".to_string(),
        "status".to_string(),
        "risk".to_string(),
        "tokens".to_string(),
        "context".to_string(),
        "path".to_string(),
        "environment".to_string(),
    ]
}

fn default_true() -> bool {
    true
}

fn default_cooldown_minutes() -> u64 {
    10
}

fn default_refresh_interval_seconds() -> u64 {
    3
}

fn default_enabled_agents() -> Vec<String> {
    vec![
        "Claude Code".to_string(),
        "Codex".to_string(),
        "OpenCode".to_string(),
    ]
}

fn default_context_warning_percent() -> f64 {
    85.0
}

fn default_context_critical_percent() -> f64 {
    95.0
}

fn default_stalled_warning_minutes() -> u64 {
    15
}

fn default_stalled_critical_minutes() -> u64 {
    30
}

fn default_token_warning_threshold() -> u64 {
    1_000_000
}

fn default_history_retention_days() -> u64 {
    30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_clamps_thresholds_and_dedupes_lists() {
        let settings = AppSettings {
            cooldown_minutes: 0,
            refresh_interval_seconds: 1,
            launch_at_login: true,
            enabled_agents: vec![
                " Codex ".to_string(),
                "codex".to_string(),
                "".to_string(),
                "Claude Code".to_string(),
            ],
            hidden_projects: vec![
                " demo ".to_string(),
                "DEMO".to_string(),
                "".to_string(),
                "archive".to_string(),
            ],
            claude_data_roots: vec![
                " ~/.claude ".to_string(),
                "~/.claude".to_string(),
                "".to_string(),
                "/tmp/claude-alt".to_string(),
            ],
            codex_data_roots: vec![" ~/.codex ".to_string(), "~/.codex".to_string()],
            opencode_data_roots: vec![
                " ~/.local/share/opencode ".to_string(),
                "~/.local/share/opencode".to_string(),
            ],
            path_display_mode: "unknown".to_string(),
            remote_preview_fields: vec![
                "status".to_string(),
                "unknown".to_string(),
                "status".to_string(),
                "tokens".to_string(),
            ],
            context_warning_percent: 20.0,
            context_critical_percent: 40.0,
            stalled_warning_minutes: 1,
            stalled_critical_minutes: 2,
            token_warning_threshold: 1,
            history_retention_days: 0,
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.cooldown_minutes, 1);
        assert_eq!(settings.refresh_interval_seconds, 2);
        assert!(settings.launch_at_login);
        assert_eq!(settings.enabled_agents, vec!["Codex", "Claude Code"]);
        assert_eq!(settings.hidden_projects, vec!["demo", "archive"]);
        assert_eq!(
            settings.claude_data_roots,
            vec!["~/.claude", "/tmp/claude-alt"]
        );
        assert_eq!(settings.codex_data_roots, vec!["~/.codex"]);
        assert_eq!(
            settings.opencode_data_roots,
            vec!["~/.local/share/opencode"]
        );
        assert_eq!(settings.path_display_mode, "compact");
        assert_eq!(settings.remote_preview_fields, vec!["status", "tokens"]);
        assert_eq!(settings.context_warning_percent, 50.0);
        assert_eq!(settings.context_critical_percent, 51.0);
        assert_eq!(settings.stalled_warning_minutes, 3);
        assert_eq!(settings.stalled_critical_minutes, 4);
        assert_eq!(settings.token_warning_threshold, 10_000);
        assert_eq!(settings.history_retention_days, 1);
    }

    #[test]
    fn agent_and_hidden_project_matching_is_case_insensitive() {
        let settings = AppSettings {
            enabled_agents: vec!["codex".to_string()],
            hidden_projects: vec!["Secret-App".to_string(), "/tmp/archive".to_string()],
            ..AppSettings::default()
        };

        assert!(settings.agent_enabled("Codex"));
        assert!(!settings.agent_enabled("Claude Code"));
        assert!(settings.hides_session("secret-app", "/Users/test/secret-app"));
        assert!(settings.hides_session("frontend", "/TMP/ARCHIVE/frontend"));
        assert!(!settings.hides_session("frontend", "/Users/test/frontend"));
    }

    #[test]
    fn plan_is_normalized_to_free_or_pro() {
        let free = AppSettings {
            plan: "team".to_string(),
            ..AppSettings::default()
        }
        .normalized();
        let pro = AppSettings {
            plan: "PRO".to_string(),
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(free.plan, "free");
        assert!(!free.is_pro());
        assert_eq!(pro.plan, "pro");
        assert!(pro.is_pro());
    }

    #[test]
    fn data_roots_include_default_and_custom_paths() {
        let settings = AppSettings {
            claude_data_roots: vec!["~/custom-claude".to_string()],
            codex_data_roots: vec!["/tmp/custom-codex".to_string()],
            opencode_data_roots: vec!["/tmp/custom-opencode".to_string()],
            ..AppSettings::default()
        };

        let claude_roots = settings.claude_data_roots();
        let codex_roots = settings.codex_data_roots();
        let opencode_roots = settings.opencode_data_roots();

        assert!(claude_roots.iter().any(|path| path.ends_with(".claude")));
        assert!(claude_roots
            .iter()
            .any(|path| path.ends_with("custom-claude")));
        assert!(codex_roots.iter().any(|path| path.ends_with(".codex")));
        assert!(codex_roots
            .iter()
            .any(|path| path == &PathBuf::from("/tmp/custom-codex")));
        assert!(opencode_roots
            .iter()
            .any(|path| path.ends_with(".local/share/opencode")));
        assert!(opencode_roots
            .iter()
            .any(|path| path == &PathBuf::from("/tmp/custom-opencode")));
    }

    #[test]
    fn remote_preview_fields_fall_back_when_empty_or_invalid() {
        let settings = AppSettings {
            remote_preview_fields: vec!["prompt".to_string(), "secret".to_string()],
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(
            settings.remote_preview_fields,
            default_remote_preview_fields()
        );
    }

    #[test]
    fn remote_preview_fields_allow_locked_pro_timeline_selection() {
        let settings = AppSettings {
            remote_preview_fields: vec![
                "status".to_string(),
                "timeline".to_string(),
                "environment".to_string(),
            ],
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(
            settings.remote_preview_fields,
            vec!["status", "timeline", "environment"]
        );
        assert!(!default_remote_preview_fields().contains(&"timeline".to_string()));
    }
}
