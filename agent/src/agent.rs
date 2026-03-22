//! Agent loop: orchestrates API calls and tool dispatch.

use anyhow::Result;

/// Run a single agent task to completion, sending IPC responses along the way.
/// TODO: implement multi-turn loop with tool use
pub async fn run_task(_task_id: String, _prompt: String) -> Result<()> {
    // 1. Build initial message list
    // 2. Call api::AnthropicClient::stream_message
    // 3. Stream tokens back via ipc::send(Response::Token)
    // 4. If model requests a tool call: ipc::send(Response::ToolCall),
    //    dispatch to tools::execute, ipc::send(Response::ToolResult)
    // 5. Re-prompt with tool result, loop until no more tool calls
    // 6. ipc::send(Response::Done)
    Ok(())
}
