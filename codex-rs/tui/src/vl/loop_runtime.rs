//! Custom loop payload model for codex-vl.
//!
//! Existing loop jobs store a raw prompt in `prompt_text`. The v1 envelope
//! keeps that storage backward compatible: raw text remains a prompt payload,
//! while structured payloads are stored with a small tagged prefix.

const STORAGE_PREFIX: &str = "__codex_vl_loop_payload_v1:";

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum LoopJobPayload {
    Prompt {
        text: String,
    },
    InternalFn {
        fn_name: String,
        #[serde(default)]
        args: serde_json::Value,
    },
}

impl LoopJobPayload {
    pub(crate) fn prompt(text: impl Into<String>) -> Self {
        Self::Prompt { text: text.into() }
    }

    pub(crate) fn from_storage_text(text: &str) -> Self {
        let Some(encoded) = text.strip_prefix(STORAGE_PREFIX) else {
            return Self::prompt(text.to_string());
        };
        serde_json::from_str(encoded).unwrap_or_else(|_| Self::prompt(text.to_string()))
    }

    pub(crate) fn to_storage_text(&self) -> anyhow::Result<String> {
        match self {
            Self::Prompt { text } => Ok(text.clone()),
            Self::InternalFn { .. } => Ok(format!(
                "{STORAGE_PREFIX}{}",
                serde_json::to_string(self).map_err(|err| anyhow::anyhow!(err))?
            )),
        }
    }

    pub(crate) fn from_tool_payload(
        raw_payload: Option<serde_json::Value>,
        fallback_prompt: Option<String>,
    ) -> anyhow::Result<Self> {
        match raw_payload {
            None => {
                let prompt = fallback_prompt
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("`prompt` is required for add"))?;
                Ok(Self::prompt(prompt))
            }
            Some(raw_payload) => {
                let object = raw_payload
                    .as_object()
                    .ok_or_else(|| anyhow::anyhow!("`payload` must be an object"))?;
                let kind = object
                    .get("type")
                    .or_else(|| object.get("kind"))
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow::anyhow!("`payload.type` is required"))?
                    .trim()
                    .to_ascii_lowercase();
                match kind.as_str() {
                    "prompt" => {
                        let text = object
                            .get("text")
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                            .or(fallback_prompt)
                            .filter(|value| !value.trim().is_empty())
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "`payload.text` or `prompt` is required for prompt payload"
                                )
                            })?;
                        Ok(Self::prompt(text))
                    }
                    "internal_fn" => {
                        let fn_name = object
                            .get("fn_name")
                            .or_else(|| object.get("name"))
                            .and_then(|value| value.as_str())
                            .ok_or_else(|| {
                                anyhow::anyhow!("`payload.fn_name` is required for internal_fn")
                            })?
                            .trim()
                            .to_string();
                        validate_internal_fn_name(&fn_name)?;
                        let args = object
                            .get("args")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({}));
                        if !args.is_object() {
                            return Err(anyhow::anyhow!("`payload.args` must be an object"));
                        }
                        Ok(Self::InternalFn { fn_name, args })
                    }
                    "mcp_tool" => Err(anyhow::anyhow!(
                        "`payload.type=mcp_tool` is not enabled in this build"
                    )),
                    other => Err(anyhow::anyhow!("unsupported loop payload type `{other}`")),
                }
            }
        }
    }

    pub(crate) fn prompt_text(&self) -> Option<&str> {
        match self {
            Self::Prompt { text } => Some(text),
            Self::InternalFn { .. } => None,
        }
    }

    pub(crate) fn display_text(&self) -> String {
        match self {
            Self::Prompt { text } => text.clone(),
            Self::InternalFn { fn_name, args } if args == &serde_json::json!({}) => {
                format!("internal_fn:{fn_name}")
            }
            Self::InternalFn { fn_name, args } => {
                format!("internal_fn:{fn_name} {}", args)
            }
        }
    }

    pub(crate) fn to_public_json(&self) -> serde_json::Value {
        match self {
            Self::Prompt { text } => serde_json::json!({
                "type": "prompt",
                "text": text,
            }),
            Self::InternalFn { fn_name, args } => serde_json::json!({
                "type": "internal_fn",
                "fn_name": fn_name,
                "args": args,
            }),
        }
    }
}

pub(crate) fn validate_internal_fn_name(fn_name: &str) -> anyhow::Result<()> {
    match fn_name {
        "loop.status" | "loop.noop" => Ok(()),
        other => Err(anyhow::anyhow!(
            "unsupported internal loop function `{other}`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::LoopJobPayload;

    #[test]
    fn raw_storage_text_is_prompt_payload() {
        let payload = LoopJobPayload::from_storage_text("check forge");

        assert_eq!(payload, LoopJobPayload::prompt("check forge"));
        assert_eq!(payload.to_storage_text().unwrap(), "check forge");
    }

    #[test]
    fn internal_fn_storage_round_trips() {
        let payload = LoopJobPayload::InternalFn {
            fn_name: "loop.status".to_string(),
            args: serde_json::json!({"message": "still watching"}),
        };

        let storage = payload.to_storage_text().unwrap();
        assert!(storage.starts_with(super::STORAGE_PREFIX));
        assert_eq!(LoopJobPayload::from_storage_text(&storage), payload);
    }

    #[test]
    fn mcp_tool_payload_is_gated_off() {
        let error = LoopJobPayload::from_tool_payload(
            Some(serde_json::json!({
                "type": "mcp_tool",
                "server": "memory",
                "tool": "memory_read",
                "args": {}
            })),
            None,
        )
        .expect_err("mcp tool payload should be disabled");

        assert!(error.to_string().contains("not enabled"));
    }
}
