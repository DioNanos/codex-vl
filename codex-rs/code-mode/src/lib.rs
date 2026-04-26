mod description;
mod response;
#[cfg(not(target_os = "android"))]
mod runtime;
#[cfg(target_os = "android")]
mod runtime_stub;
#[cfg(not(target_os = "android"))]
mod service;
#[cfg(target_os = "android")]
mod service_stub;

pub use description::CODE_MODE_PRAGMA_PREFIX;
pub use description::CodeModeToolKind;
pub use description::ToolDefinition;
pub use description::ToolNamespaceDescription;
pub use description::augment_tool_definition;
pub use description::build_exec_tool_description;
pub use description::build_wait_tool_description;
pub use description::is_code_mode_nested_tool;
pub use description::normalize_code_mode_identifier;
pub use description::parse_exec_source;
pub use description::render_code_mode_sample;
pub use description::render_json_schema_to_typescript;
pub use response::DEFAULT_IMAGE_DETAIL;
pub use response::FunctionCallOutputContentItem;
pub use response::ImageDetail;
#[cfg(not(target_os = "android"))]
pub use runtime::CodeModeNestedToolCall;
#[cfg(not(target_os = "android"))]
pub use runtime::DEFAULT_EXEC_YIELD_TIME_MS;
#[cfg(not(target_os = "android"))]
pub use runtime::DEFAULT_MAX_OUTPUT_TOKENS_PER_EXEC_CALL;
#[cfg(not(target_os = "android"))]
pub use runtime::DEFAULT_WAIT_YIELD_TIME_MS;
#[cfg(not(target_os = "android"))]
pub use runtime::ExecuteRequest;
#[cfg(not(target_os = "android"))]
pub use runtime::RuntimeResponse;
#[cfg(not(target_os = "android"))]
pub use runtime::WaitOutcome;
#[cfg(not(target_os = "android"))]
pub use runtime::WaitRequest;
#[cfg(target_os = "android")]
pub use runtime_stub::CodeModeNestedToolCall;
#[cfg(target_os = "android")]
pub use runtime_stub::DEFAULT_EXEC_YIELD_TIME_MS;
#[cfg(target_os = "android")]
pub use runtime_stub::DEFAULT_MAX_OUTPUT_TOKENS_PER_EXEC_CALL;
#[cfg(target_os = "android")]
pub use runtime_stub::DEFAULT_WAIT_YIELD_TIME_MS;
#[cfg(target_os = "android")]
pub use runtime_stub::ExecuteRequest;
#[cfg(target_os = "android")]
pub use runtime_stub::RuntimeResponse;
#[cfg(target_os = "android")]
pub use runtime_stub::WaitOutcome;
#[cfg(target_os = "android")]
pub use runtime_stub::WaitRequest;
#[cfg(not(target_os = "android"))]
pub use service::CodeModeService;
#[cfg(not(target_os = "android"))]
pub use service::CodeModeTurnHost;
#[cfg(not(target_os = "android"))]
pub use service::CodeModeTurnWorker;
#[cfg(target_os = "android")]
pub use service_stub::CodeModeService;
#[cfg(target_os = "android")]
pub use service_stub::CodeModeTurnHost;
#[cfg(target_os = "android")]
pub use service_stub::CodeModeTurnWorker;

pub const PUBLIC_TOOL_NAME: &str = "exec";
pub const WAIT_TOOL_NAME: &str = "wait";
