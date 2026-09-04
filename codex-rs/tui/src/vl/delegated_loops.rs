use codex_state::LoopDelegation;
use codex_state::ThreadLoopOwner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VivlingReadiness {
    Runnable,
    WrapperUnavailable,
    NotAdult,
    BrainDisabled,
    NotRequested,
}

impl VivlingReadiness {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Runnable => "adult+brain",
            Self::WrapperUnavailable => "unavailable",
            Self::NotAdult => "not_adult",
            Self::BrainDisabled => "brain_disabled",
            Self::NotRequested => "not_requested",
        }
    }

    pub(crate) const fn is_runnable(self) -> bool {
        matches!(self, Self::Runnable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RequestedLoopOwner {
    Main,
    Vivling { vivling_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopOwnerSource {
    Delegation,
    ThreadOwner,
}

impl LoopOwnerSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Delegation => "delegation",
            Self::ThreadOwner => "owner",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectiveLoopOwner {
    pub(crate) requested: RequestedLoopOwner,
    pub(crate) effective: RequestedLoopOwner,
    pub(crate) source: LoopOwnerSource,
    pub(crate) readiness: VivlingReadiness,
    pub(crate) reason: &'static str,
}

pub(crate) fn resolve_effective_owner(
    delegation: Option<&LoopDelegation>,
    thread_owner: &ThreadLoopOwner,
    readiness: VivlingReadiness,
) -> EffectiveLoopOwner {
    let (requested, source) = if let Some(delegation) = delegation {
        if delegation.override_main {
            (RequestedLoopOwner::Main, LoopOwnerSource::Delegation)
        } else {
            (
                RequestedLoopOwner::Vivling {
                    vivling_id: delegation.vivling_id.clone(),
                },
                LoopOwnerSource::Delegation,
            )
        }
    } else if thread_owner.owner_kind == codex_state::THREAD_LOOP_OWNER_KIND_VIVLING {
        match thread_owner.owner_vivling_id.as_deref() {
            Some(vivling_id) => (
                RequestedLoopOwner::Vivling {
                    vivling_id: vivling_id.to_string(),
                },
                LoopOwnerSource::ThreadOwner,
            ),
            None => (RequestedLoopOwner::Main, LoopOwnerSource::ThreadOwner),
        }
    } else {
        (RequestedLoopOwner::Main, LoopOwnerSource::ThreadOwner)
    };

    let (effective, reason) = match (&requested, readiness) {
        (RequestedLoopOwner::Main, _) if delegation.is_some() => {
            (RequestedLoopOwner::Main, "hard_main_override")
        }
        (RequestedLoopOwner::Main, _) => (RequestedLoopOwner::Main, "not_delegated"),
        (RequestedLoopOwner::Vivling { .. }, readiness)
            if readiness.is_runnable() && source == LoopOwnerSource::Delegation =>
        {
            (requested.clone(), "delegated_and_runnable")
        }
        (RequestedLoopOwner::Vivling { .. }, readiness)
            if readiness.is_runnable() && source == LoopOwnerSource::ThreadOwner =>
        {
            (requested.clone(), "not_delegated")
        }
        (RequestedLoopOwner::Vivling { .. }, _) => {
            (RequestedLoopOwner::Main, "vivling_not_runnable")
        }
    };

    EffectiveLoopOwner {
        requested,
        effective,
        source,
        readiness,
        reason,
    }
}

pub(crate) fn owner_from_resolution(
    resolution: &EffectiveLoopOwner,
    thread_id: codex_protocol::ThreadId,
    updated_at_ms: i64,
) -> ThreadLoopOwner {
    match &resolution.effective {
        RequestedLoopOwner::Main => ThreadLoopOwner {
            thread_id,
            owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
            owner_vivling_id: None,
            updated_at_ms,
        },
        RequestedLoopOwner::Vivling { vivling_id } => ThreadLoopOwner {
            thread_id,
            owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_VIVLING.to_string(),
            owner_vivling_id: Some(vivling_id.clone()),
            updated_at_ms,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::ThreadId;

    fn owner(kind: &str, vivling_id: Option<&str>) -> ThreadLoopOwner {
        ThreadLoopOwner {
            thread_id: ThreadId::new(),
            owner_kind: kind.to_string(),
            owner_vivling_id: vivling_id.map(str::to_string),
            updated_at_ms: 0,
        }
    }

    fn delegation(override_main: bool) -> LoopDelegation {
        LoopDelegation {
            thread_id: ThreadId::new(),
            job_id: "job-1".to_string(),
            loop_label: "ci".to_string(),
            vivling_id: "vivling-1".to_string(),
            strategy: codex_state::LoopDelegationStrategy::Observe,
            ticks_managed: 0,
            recent_results_json: "[]".to_string(),
            last_plan_approved: None,
            strategy_override: None,
            override_main,
            cooldown_until_ms: None,
            suspend_reason: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn delegation_precedes_thread_owner_when_runnable() {
        let resolved = resolve_effective_owner(
            Some(&delegation(false)),
            &owner(codex_state::THREAD_LOOP_OWNER_KIND_MAIN, None),
            VivlingReadiness::Runnable,
        );
        assert_eq!(
            resolved.effective,
            RequestedLoopOwner::Vivling {
                vivling_id: "vivling-1".to_string()
            }
        );
        assert_eq!(resolved.reason, "delegated_and_runnable");
    }

    #[test]
    fn not_delegated_preserves_thread_owner_and_hard_main_wins() {
        let resolved = resolve_effective_owner(
            None,
            &owner(
                codex_state::THREAD_LOOP_OWNER_KIND_VIVLING,
                Some("vivling-1"),
            ),
            VivlingReadiness::Runnable,
        );
        assert_eq!(resolved.effective, resolved.requested);
        assert_eq!(resolved.reason, "not_delegated");

        let hard_main = resolve_effective_owner(
            Some(&delegation(true)),
            &owner(
                codex_state::THREAD_LOOP_OWNER_KIND_VIVLING,
                Some("vivling-1"),
            ),
            VivlingReadiness::Runnable,
        );
        assert_eq!(hard_main.effective, RequestedLoopOwner::Main);
        assert_eq!(hard_main.reason, "hard_main_override");
    }

    #[test]
    fn non_runnable_vivling_falls_back_to_main_without_deleting_state() {
        let resolved = resolve_effective_owner(
            Some(&delegation(false)),
            &owner(codex_state::THREAD_LOOP_OWNER_KIND_MAIN, None),
            VivlingReadiness::NotAdult,
        );
        assert_eq!(resolved.effective, RequestedLoopOwner::Main);
        assert_eq!(resolved.reason, "vivling_not_runnable");
        assert_eq!(resolved.readiness, VivlingReadiness::NotAdult);
    }
}
