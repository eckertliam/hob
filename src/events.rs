//! Event system: channels between the agent loop and the TUI.
//!
//! Replaces the JSON IPC layer. The agent sends UiEvents to the TUI,
//! and the TUI sends UserActions to the agent.

use tokio::sync::mpsc;

/// Events sent from the agent to the TUI for display.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Streaming text token from the model.
    Token { id: String, content: String },
    /// A tool is being invoked.
    ToolCall {
        id: String,
        tool: String,
        input: serde_json::Value,
    },
    /// A tool has completed.
    ToolResult {
        id: String,
        tool: String,
        output: String,
        is_error: bool,
    },
    /// Task completed successfully.
    Done {
        id: String,
        input_tokens: u32,
        output_tokens: u32,
    },
    /// An error occurred.
    Error { id: String, message: String },
    /// Status update (retry, busy, etc).
    Status { id: String, message: String },
    /// Request permission from the user.
    PermissionRequest {
        id: String,
        request_id: String,
        tool: String,
        resource: String,
    },
}

/// Actions sent from the TUI to the agent.
#[derive(Debug, Clone)]
pub enum UserAction {
    /// Send a new prompt, optionally with an image attachment.
    Task {
        id: String,
        prompt: String,
        image: Option<(String, String)>, // (media_type, base64_data)
    },
    /// Cancel the current task.
    Cancel { id: String },
    /// Respond to a permission request.
    PermissionResponse {
        request_id: String,
        decision: PermissionDecision,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Once,
    Always,
    Reject,
}

/// Create the event channels.
pub fn create_channels() -> (EventSender, EventReceiver, ActionSender, ActionReceiver) {
    let (ui_tx, ui_rx) = mpsc::channel(256);
    let (action_tx, action_rx) = mpsc::channel(64);
    (
        EventSender(ui_tx),
        EventReceiver(ui_rx),
        ActionSender(action_tx),
        ActionReceiver(action_rx),
    )
}

/// Sender for UI events (agent → TUI).
#[derive(Clone)]
pub struct EventSender(pub mpsc::Sender<UiEvent>);

impl EventSender {
    pub async fn send(&self, event: UiEvent) {
        let _ = self.0.send(event).await;
    }
}

/// Receiver for UI events (TUI reads these).
pub struct EventReceiver(pub mpsc::Receiver<UiEvent>);

/// Sender for user actions (TUI → agent).
#[derive(Clone)]
pub struct ActionSender(pub mpsc::Sender<UserAction>);

impl ActionSender {
    pub async fn send(&self, action: UserAction) {
        let _ = self.0.send(action).await;
    }
}

/// Receiver for user actions (agent reads these).
pub struct ActionReceiver(pub mpsc::Receiver<UserAction>);
