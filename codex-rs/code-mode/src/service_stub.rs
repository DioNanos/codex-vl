use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use tokio_util::sync::CancellationToken;

use crate::ExecuteRequest;
use crate::RuntimeResponse;
use crate::WaitRequest;

const ANDROID_CODE_MODE_UNSUPPORTED: &str = "exec is not supported in the Android Termux build";

#[async_trait]
pub trait CodeModeTurnHost: Send + Sync {
    async fn invoke_tool(
        &self,
        tool_name: String,
        input: Option<JsonValue>,
        cancellation_token: CancellationToken,
    ) -> Result<JsonValue, String>;

    async fn notify(&self, call_id: String, cell_id: String, text: String) -> Result<(), String>;
}

pub struct CodeModeService;

impl CodeModeService {
    pub fn new() -> Self {
        Self
    }

    pub async fn stored_values(&self) -> HashMap<String, JsonValue> {
        HashMap::new()
    }

    pub async fn replace_stored_values(&self, _values: HashMap<String, JsonValue>) {}

    pub async fn execute(&self, request: ExecuteRequest) -> Result<RuntimeResponse, String> {
        Ok(RuntimeResponse::Result {
            cell_id: "android-termux".to_string(),
            content_items: Vec::new(),
            stored_values: request.stored_values,
            error_text: Some(ANDROID_CODE_MODE_UNSUPPORTED.to_string()),
        })
    }

    pub async fn wait(&self, request: WaitRequest) -> Result<RuntimeResponse, String> {
        Ok(RuntimeResponse::Result {
            cell_id: request.cell_id,
            content_items: Vec::new(),
            stored_values: HashMap::new(),
            error_text: Some(ANDROID_CODE_MODE_UNSUPPORTED.to_string()),
        })
    }

    pub fn start_turn_worker(&self, _host: Arc<dyn CodeModeTurnHost>) -> CodeModeTurnWorker {
        CodeModeTurnWorker
    }
}

impl Default for CodeModeService {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CodeModeTurnWorker;
