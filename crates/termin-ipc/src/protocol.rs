//! IPC JSON-RPC protocol types and message definitions.
//!
//! See `docs/specs/ipc-cli.md §2, §3`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

// Standard method names
pub const METHOD_PING: &str = "ping";
pub const METHOD_LIST_WINDOWS: &str = "list_windows";
pub const METHOD_LIST_WORKSPACES: &str = "list_workspaces";
pub const METHOD_LIST_PANES: &str = "list_panes";
pub const METHOD_NEW_WORKSPACE: &str = "new_workspace";
pub const METHOD_SELECT_WORKSPACE: &str = "select_workspace";
pub const METHOD_RENAME_WORKSPACE: &str = "rename_workspace";
pub const METHOD_CLOSE_WORKSPACE: &str = "close_workspace";
pub const METHOD_NEW_TAB: &str = "new_tab";
pub const METHOD_SPLIT: &str = "split";
pub const METHOD_FOCUS: &str = "focus";
pub const METHOD_CLOSE: &str = "close";
pub const METHOD_SEND_TEXT: &str = "send_text";
pub const METHOD_SEND_KEYS: &str = "send_keys";
pub const METHOD_READ_SCREEN: &str = "read_screen";
pub const METHOD_NOTIFY: &str = "notify";
pub const METHOD_SET_STATUS: &str = "set_status";
pub const METHOD_RUN_ACTION: &str = "run_action";
pub const METHOD_SUBSCRIBE: &str = "subscribe";

/// Incoming IPC JSON-RPC request.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl Request {
    /// Create a new request with the given method and serialized params.
    pub fn new(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            id,
            method: method.into(),
            params,
        }
    }

    /// Attempt to deserialize params into a typed struct.
    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.params.clone())
    }
}

/// Error details returned in an IPC response.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: String,
    pub message: String,
}

impl ResponseError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new("invalid_params", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}

/// Outgoing IPC JSON-RPC response.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Response {
    /// Create a successful response with a result value.
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response with error code and message.
    pub fn error(id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(ResponseError::new(code, message)),
        }
    }

    /// Check if response represents success.
    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }

    /// Check if response represents an error.
    pub fn is_err(&self) -> bool {
        self.error.is_some()
    }

    /// Attempt to deserialize result into a typed struct.
    pub fn result_as<T: DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        match &self.result {
            Some(res) => serde_json::from_value(res.clone()).map(Some),
            None => Ok(None),
        }
    }
}

/// Outgoing asynchronous event emitted after subscription.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub data: serde_json::Value,
}

impl Event {
    pub fn new(event: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            event: event.into(),
            data,
        }
    }

    pub fn data_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.data.clone())
    }
}

// ---------------------------------------------------------------------------
// Typed method parameter & result payloads (docs/specs/ipc-cli.md §3)
// ---------------------------------------------------------------------------

/// Result for `ping` method.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResult {
    pub version: String,
}

/// Window description returned by `list_windows`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub workspaces: Vec<u32>,
    pub active: bool,
}

/// Workspace description returned by `list_workspaces`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: u32,
    pub name: String,
    pub window: u32,
    pub panes: Vec<u32>,
    pub attention: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

/// Tab description in `PaneInfo`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: u32,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg_process: Option<String>,
    pub mode: String,
}

/// Pane description returned by `list_panes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: u32,
    pub tabs: Vec<TabInfo>,
    pub focused: bool,
}

/// Params for `new_workspace`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWorkspaceParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Result for `new_workspace`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWorkspaceResult {
    pub workspace: u32,
    pub pane: u32,
    pub tab: u32,
}

/// Params for `select_workspace`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectWorkspaceParams {
    pub workspace: u32,
}

/// Params for `rename_workspace`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameWorkspaceParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<u32>,
    pub name: String,
}

/// Params for `close_workspace`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseWorkspaceParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<u32>,
}

/// Params for `new_tab`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTabParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Result for `new_tab`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTabResult {
    pub tab: u32,
}

/// Split direction for `split`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

/// Params for `split`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
    pub direction: SplitDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Result for `split`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitResult {
    pub pane: u32,
    pub tab: u32,
}

/// Params for `focus`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u32>,
}

/// Params for `close`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u32>,
}

/// Params for `send_text`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendTextParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u32>,
    pub text: String,
}

/// Params for `send_keys`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendKeysParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u32>,
    pub keys: Vec<String>,
}

/// Params for `read_screen`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadScreenParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<bool>,
}

/// Result for `read_screen`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadScreenResult {
    pub text: String,
}

/// Params for `notify`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub body: String,
}

/// Params for `set_status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetStatusParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<u32>,
    pub text: String,
}

/// Params for `run_action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunActionParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

/// Params for `subscribe`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeParams {
    pub events: Vec<String>,
}
