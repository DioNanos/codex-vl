#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct VivlingLiveStatusItem {
    pub(crate) id: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct VivlingLiveContext {
    pub(crate) status_items: Vec<VivlingLiveStatusItem>,
    pub(crate) run_state: Option<String>,
    pub(crate) active_agent_label: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) thread_title: Option<String>,
    pub(crate) task_progress: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) git_branch: Option<String>,
}

impl VivlingLiveContext {
    pub(crate) fn crt_phrase(&self) -> Option<String> {
        if let Some(label) = self.active_agent_label.as_deref().and_then(clean_value) {
            return Some(format!("active: {}", truncate_chars(label, 20)));
        }

        if let Some(progress) = self.task_progress.as_deref().and_then(clean_value) {
            return Some(truncate_chars(progress, 28));
        }

        if let Some(run_state) = self.run_state.as_deref().and_then(clean_value) {
            if run_state != "Ready" {
                return Some(run_state.to_ascii_lowercase());
            }
        }

        if let Some(branch) = self.git_branch.as_deref().and_then(clean_value) {
            return Some(format!("branch {}", truncate_chars(branch, 20)));
        }

        self.cwd
            .as_deref()
            .and_then(clean_value)
            .map(|cwd| format!("watching {}", truncate_chars(last_path_component(cwd), 18)))
    }

    pub(crate) fn memory_summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        push_part(&mut parts, "state", self.run_state.as_deref());
        push_part(&mut parts, "active", self.active_agent_label.as_deref());
        push_part(&mut parts, "task", self.task_progress.as_deref());
        push_part(&mut parts, "branch", self.git_branch.as_deref());
        push_part(
            &mut parts,
            "cwd",
            self.cwd.as_deref().map(last_path_component),
        );

        if parts.is_empty() {
            None
        } else {
            Some(truncate_chars(&parts.join("; "), 160))
        }
    }
}

fn push_part(parts: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.and_then(clean_value) {
        parts.push(format!("{label} {value}"));
    }
}

fn clean_value(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn last_path_component(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(2);
    let mut out: String = value.chars().take(keep).collect();
    out.push_str("..");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_agent_label_drives_crt_phrase() {
        let context = VivlingLiveContext {
            active_agent_label: Some("Robie [worker]".to_string()),
            run_state: Some("Working".to_string()),
            ..Default::default()
        };

        assert_eq!(
            context.crt_phrase(),
            Some("active: Robie [worker]".to_string())
        );
    }

    #[test]
    fn memory_summary_is_compact_and_labeled() {
        let context = VivlingLiveContext {
            run_state: Some("Working".to_string()),
            active_agent_label: Some("main".to_string()),
            cwd: Some("/home/dag/Dev/60_toolchains/codex-vl".to_string()),
            ..Default::default()
        };

        let summary = context.memory_summary().expect("summary");
        assert!(summary.contains("state Working"));
        assert!(summary.contains("active main"));
        assert!(summary.contains("cwd codex-vl"));
    }

    // The Vivling::set_live_context short-circuit relies on PartialEq over every
    // observable field. If a new field is added without being compared, redundant
    // status syncs will silently keep requesting frames.
    #[test]
    fn equal_contexts_compare_equal_across_all_fields() {
        let context = VivlingLiveContext {
            status_items: vec![VivlingLiveStatusItem {
                id: "GitBranch".to_string(),
                value: "develop".to_string(),
            }],
            run_state: Some("Working".to_string()),
            active_agent_label: Some("main".to_string()),
            model: Some("opus-4.7".to_string()),
            cwd: Some("/tmp".to_string()),
            thread_title: Some("thread".to_string()),
            task_progress: Some("12%".to_string()),
            session_id: Some("abc".to_string()),
            git_branch: Some("develop".to_string()),
        };

        assert_eq!(context, context.clone());

        let mut diverged = context.clone();
        diverged.task_progress = Some("13%".to_string());
        assert_ne!(context, diverged);
    }
}
