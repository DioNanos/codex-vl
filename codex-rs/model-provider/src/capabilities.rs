//! Provider capability defaults and resolution of custom provider overrides.

use codex_api::is_azure_responses_provider;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::RemoteCompactionSupport;

/// Optional provider-backed features that Codex may expose at runtime.
///
/// These capabilities are a provider-owned upper bound. Model and turn settings
/// can further restrict the functionality exposed to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    // codex-vl fork: upstream removed this provider gate (#50447). The fork keeps
    // it so chat-bridge providers can ask for flat, unprefixed tool names.
    pub namespace_tools: bool,
    pub image_generation: bool,
    pub web_search: bool,
    pub external_web_access: bool,
    pub remote_compaction: RemoteCompactionSupport,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            namespace_tools: true,
            image_generation: true,
            web_search: true,
            external_web_access: true,
            remote_compaction: RemoteCompactionSupport::Unsupported,
        }
    }
}

impl ProviderCapabilities {
    pub(crate) fn from_config(info: &ModelProviderInfo) -> Self {
        let defaults = Self {
            // The provider config may omit the field; the default stays on.
            namespace_tools: info.namespace_tools.unwrap_or(true),
            remote_compaction: if info.is_openai()
                || is_azure_responses_provider(&info.name, info.base_url.as_deref())
            {
                RemoteCompactionSupport::V2
            } else {
                RemoteCompactionSupport::Unsupported
            },
            ..Self::default()
        };
        let overrides = info.capabilities.unwrap_or_default();
        Self {
            external_web_access: overrides
                .external_web_access
                .unwrap_or(defaults.external_web_access),
            remote_compaction: overrides
                .remote_compaction
                .unwrap_or(defaults.remote_compaction),
            ..defaults
        }
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
