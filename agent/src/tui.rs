//! Terminal UI using ratatui + crossterm.
//!
//! Layout:
//! ┌──────────────────────────────────┐
//! │ Chat history (scrollable)        │
//! │ You: prompt text                 │
//! │ hob: streaming response...       │
//! │ ● tool_name  ✓ result           │
//! ├──────────────────────────────────┤
//! │ Status bar                       │
//! ├──────────────────────────────────┤
//! │ > input area                     │
//! └──────────────────────────────────┘

use std::io;
use std::sync::Arc;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::api::Provider;
use crate::events::{
    ActionSender, EventReceiver, PermissionDecision, UiEvent, UserAction,
};
use crate::permission::{self, PendingMap};
use crate::store::Store;

/// A line in the chat history.
#[derive(Clone)]
enum ChatLine {
    UserHeader,
    UserText(String),
    AssistantHeader,
    AssistantText(String),
    ToolCall(String),
    ToolResult(String, bool),
    Status(String),
    Error(String),
    /// System message (command output, help text).
    System(String),
    Separator,
}

/// The TUI application state.
struct App {
    /// Chat history lines.
    chat: Vec<ChatLine>,
    /// Current input text.
    input: String,
    /// Cursor position in input.
    cursor: usize,
    /// Scroll offset for chat history.
    scroll: u16,
    /// Whether we're auto-scrolling.
    following: bool,
    /// Current agent status.
    status: String,
    /// Current task ID, if any.
    current_task: Option<String>,
    /// Task counter.
    task_counter: u32,
    /// Pending permission request, if any.
    pending_permission: Option<(String, String, String)>, // (request_id, tool, resource)
    /// Input history.
    history: Vec<String>,
    history_index: Option<usize>,
    /// Current model ID.
    model: String,
    /// Cumulative token usage for current session.
    total_input_tokens: u32,
    total_output_tokens: u32,
    /// Store reference for session commands.
    store: Store,
}

impl App {
    fn new(model: String, store: Store) -> Self {
        Self {
            chat: vec![],
            input: String::new(),
            cursor: 0,
            scroll: 0,
            following: true,
            status: "idle".into(),
            current_task: None,
            task_counter: 0,
            pending_permission: None,
            history: vec![],
            history_index: None,
            model,
            total_input_tokens: 0,
            total_output_tokens: 0,
            store,
        }
    }

    /// Handle a slash command. Returns true if input was a command.
    async fn handle_command(&mut self, input: &str) -> bool {
        let parts: Vec<&str> = input.trim().splitn(3, ' ').collect();
        match parts.first().copied() {
            Some("/model") => {
                if let Some(model_id) = parts.get(1) {
                    if let Some(info) = crate::models::lookup(model_id) {
                        self.model = info.id.to_string();
                        // Persist to config
                        if let Ok(mut cfg) = crate::config::Config::load() {
                            cfg.model = Some(info.id.to_string());
                            let _ = cfg.save();
                        }
                        self.chat.push(ChatLine::System(format!(
                            "Model set to {} ({}). Restart hob to apply.",
                            info.name, info.id
                        )));
                    } else {
                        self.chat.push(ChatLine::System(format!(
                            "Unknown model: {model_id}"
                        )));
                        self.show_model_list();
                    }
                } else {
                    self.show_model_list();
                }
                true
            }
            Some("/provider") => {
                if let Some(provider) = parts.get(1) {
                    if *provider == "anthropic" || *provider == "openai" {
                        if let Ok(mut cfg) = crate::config::Config::load() {
                            cfg.provider = Some(provider.to_string());
                            let _ = cfg.save();
                        }
                        self.chat.push(ChatLine::System(format!(
                            "Provider set to {provider}. Restart hob to apply."
                        )));
                    } else {
                        self.chat.push(ChatLine::System(
                            "Usage: /provider anthropic|openai".into(),
                        ));
                    }
                } else {
                    self.chat.push(ChatLine::System(
                        "Usage: /provider anthropic|openai".into(),
                    ));
                }
                true
            }
            Some("/key") => {
                if parts.len() >= 3 {
                    let provider = parts[1];
                    let key = parts[2];
                    if let Ok(mut cfg) = crate::config::Config::load() {
                        match provider {
                            "anthropic" => {
                                cfg.anthropic_api_key = Some(key.to_string());
                                let _ = cfg.save();
                                self.chat.push(ChatLine::System(
                                    "Anthropic API key saved. Restart hob to apply.".into(),
                                ));
                            }
                            "openai" => {
                                cfg.openai_api_key = Some(key.to_string());
                                let _ = cfg.save();
                                self.chat.push(ChatLine::System(
                                    "OpenAI API key saved. Restart hob to apply.".into(),
                                ));
                            }
                            _ => {
                                self.chat.push(ChatLine::System(
                                    "Usage: /key anthropic|openai <api-key>".into(),
                                ));
                            }
                        }
                    }
                } else {
                    self.chat.push(ChatLine::System(
                        "Usage: /key anthropic|openai <api-key>".into(),
                    ));
                }
                true
            }
            Some("/sessions") => {
                match self.store.list_sessions().await {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            self.chat.push(ChatLine::System("No sessions found.".into()));
                        } else {
                            let mut text = String::from("Recent sessions:\n");
                            for (i, s) in sessions.iter().take(20).enumerate() {
                                let title = if s.title.is_empty() {
                                    "(untitled)"
                                } else {
                                    &s.title
                                };
                                text.push_str(&format!(
                                    "  {}. {} — {}\n",
                                    i + 1,
                                    title,
                                    s.directory,
                                ));
                            }
                            self.chat.push(ChatLine::System(text));
                        }
                    }
                    Err(e) => {
                        self.chat.push(ChatLine::System(format!("Error: {e}")));
                    }
                }
                true
            }
            Some("/clear") => {
                self.chat.clear();
                self.total_input_tokens = 0;
                self.total_output_tokens = 0;
                true
            }
            Some("/help") => {
                self.chat.push(ChatLine::System(
                    "Commands:\n  \
                     /model [id]              — show or set model\n  \
                     /provider anthropic|openai — set provider\n  \
                     /key anthropic|openai <key> — save API key\n  \
                     /sessions                — list recent sessions\n  \
                     /clear                   — clear chat history\n  \
                     /help                    — show this help"
                        .into(),
                ));
                true
            }
            _ if input.starts_with('/') => {
                self.chat.push(ChatLine::System(format!(
                    "Unknown command: {}. Type /help for available commands.",
                    parts.first().unwrap_or(&"")
                )));
                true
            }
            _ => false,
        }
    }

    fn show_model_list(&mut self) {
        let mut text = String::from("Available models:\n");

        text.push_str("\n  Anthropic:\n");
        for m in crate::models::models_for_provider("anthropic") {
            let current = if m.id == self.model { " ← current" } else { "" };
            text.push_str(&format!("    {} ({}){}\n", m.id, m.name, current));
        }

        text.push_str("\n  OpenAI:\n");
        for m in crate::models::models_for_provider("openai") {
            let current = if m.id == self.model { " ← current" } else { "" };
            text.push_str(&format!("    {} ({}){}\n", m.id, m.name, current));
        }

        text.push_str("\n  Usage: /model <id>");
        self.chat.push(ChatLine::System(text));
    }

    fn next_task_id(&mut self) -> String {
        self.task_counter += 1;
        format!("task-{}", self.task_counter)
    }

    fn scroll_to_bottom(&mut self, visible_height: u16) {
        let total = self.chat_line_count();
        if total as u16 > visible_height {
            self.scroll = total as u16 - visible_height;
        }
    }

    fn chat_line_count(&self) -> usize {
        self.chat
            .iter()
            .map(|line| match line {
                ChatLine::AssistantText(t) | ChatLine::UserText(t) | ChatLine::System(t) => {
                    t.lines().count().max(1)
                }
                _ => 1,
            })
            .sum()
    }
}

/// Run the TUI. This is the main entry point after setup.
pub async fn run(
    provider: Arc<dyn Provider>,
    model: String,
    store: Store,
) -> anyhow::Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (ui_tx, ui_rx, action_tx, action_rx) = crate::events::create_channels();
    let pending_permissions = permission::new_pending_map();

    // Spawn agent handler
    let agent_provider = Arc::clone(&provider);
    let agent_model = model.clone();
    let agent_store = store.clone();
    let agent_pending = Arc::clone(&pending_permissions);
    let agent_ui = ui_tx.clone();

    spawn_agent_handler(
        agent_provider,
        agent_model,
        agent_store,
        agent_pending,
        agent_ui,
        action_rx,
    );

    let result = run_ui_loop(
        &mut terminal,
        ui_rx,
        action_tx,
        &pending_permissions,
        model,
        store,
    )
    .await;

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

/// Spawn the agent task handler that listens for user actions.
fn spawn_agent_handler(
    provider: Arc<dyn Provider>,
    model: String,
    store: Store,
    pending_permissions: PendingMap,
    ui: crate::events::EventSender,
    mut action_rx: crate::events::ActionReceiver,
) {
    tokio::spawn(async move {
        let mut cancel: Option<CancellationToken> = None;

        while let Some(action) = action_rx.0.recv().await {
            match action {
                UserAction::Task { id, prompt } => {
                    let token = CancellationToken::new();
                    cancel = Some(token.clone());

                    let p = Arc::clone(&provider);
                    let m = model.clone();
                    let s = store.clone();
                    let pp = Arc::clone(&pending_permissions);
                    let u = ui.clone();

                    tokio::spawn(async move {
                        if let Err(e) = crate::agent::run_task(
                            &*p, &m, id.clone(), prompt, token, &s, &pp, &u,
                        )
                        .await
                        {
                            u.send(UiEvent::Error {
                                id,
                                message: format!("{e:#}"),
                            })
                            .await;
                        }
                    });
                }
                UserAction::Cancel { id: _ } => {
                    if let Some(ref c) = cancel {
                        c.cancel();
                    }
                }
                UserAction::PermissionResponse {
                    request_id,
                    decision,
                } => {
                    let d = match decision {
                        PermissionDecision::Once => permission::Decision::Once,
                        PermissionDecision::Always => permission::Decision::Always,
                        PermissionDecision::Reject => permission::Decision::Reject,
                    };
                    permission::resolve(&pending_permissions, &request_id, d).await;
                }
            }
        }
    });
}

/// Main UI loop: handles input events and agent events.
async fn run_ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut ui_rx: EventReceiver,
    action_tx: ActionSender,
    _pending_permissions: &PendingMap,
    model: String,
    store: Store,
) -> anyhow::Result<()> {
    let app = Arc::new(Mutex::new(App::new(model, store)));

    // Show welcome message
    {
        let mut app = app.lock().await;
        app.chat.push(ChatLine::System(
            "hob — terminal AI coding agent\n\
             Type a prompt and press Enter. /help for commands.\n\
             Escape to cancel, Ctrl-C to quit."
                .into(),
        ));
        app.chat.push(ChatLine::Separator);
    }

    loop {
        // Draw
        {
            let app = app.lock().await;
            terminal.draw(|f| draw(f, &app))?;
        }

        // Poll for events with a short timeout so we can check agent events
        let has_terminal_event = tokio::task::spawn_blocking(|| {
            event::poll(std::time::Duration::from_millis(16)).unwrap_or(false)
        })
        .await?;

        if has_terminal_event {
            let evt = tokio::task::spawn_blocking(|| event::read()).await??;

            match evt {
                Event::Key(key) => {
                    let mut app = app.lock().await;

                    // Handle permission prompt first
                    if let Some((ref req_id, _, _)) = app.pending_permission {
                        let decision = match key.code {
                            KeyCode::Char('y') => Some(PermissionDecision::Once),
                            KeyCode::Char('!') => Some(PermissionDecision::Always),
                            KeyCode::Char('n') | KeyCode::Esc => {
                                Some(PermissionDecision::Reject)
                            }
                            _ => None,
                        };
                        if let Some(d) = decision {
                            let req_id = req_id.clone();
                            app.pending_permission = None;
                            action_tx
                                .send(UserAction::PermissionResponse {
                                    request_id: req_id,
                                    decision: d,
                                })
                                .await;
                        }
                        continue;
                    }

                    match key {
                        // Escape: cancel current task
                        KeyEvent {
                            code: KeyCode::Esc, ..
                        } => {
                            if let Some(ref id) = app.current_task {
                                action_tx
                                    .send(UserAction::Cancel { id: id.clone() })
                                    .await;
                            }
                        }
                        // Ctrl-C / Ctrl-D: cancel or quit
                        KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }
                        | KeyEvent {
                            code: KeyCode::Char('d'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        } => {
                            if app.current_task.is_some() {
                                if let Some(ref id) = app.current_task {
                                    action_tx
                                        .send(UserAction::Cancel { id: id.clone() })
                                        .await;
                                }
                            } else {
                                return Ok(());
                            }
                        }
                        // Enter: send input or handle slash command
                        KeyEvent {
                            code: KeyCode::Enter,
                            modifiers: KeyModifiers::NONE,
                            ..
                        } => {
                            if !app.input.trim().is_empty() {
                                let input = app.input.trim().to_string();
                                app.history.push(input.clone());
                                app.history_index = None;
                                app.input.clear();
                                app.cursor = 0;
                                app.following = true;

                                // Check for slash commands
                                if app.handle_command(&input).await {
                                    // Command handled, don't send as prompt
                                } else {
                                    // Regular prompt
                                    let id = app.next_task_id();
                                    app.current_task = Some(id.clone());
                                    app.status = "streaming".into();

                                    app.chat.push(ChatLine::Separator);
                                    app.chat.push(ChatLine::UserHeader);
                                    app.chat.push(ChatLine::UserText(input.clone()));
                                    app.chat.push(ChatLine::AssistantHeader);

                                    action_tx
                                        .send(UserAction::Task { id, prompt: input })
                                        .await;
                                }
                            }
                        }
                        // Backspace
                        KeyEvent {
                            code: KeyCode::Backspace,
                            ..
                        } => {
                            if app.cursor > 0 {
                                app.cursor -= 1;
                                let pos = app.cursor;
                                app.input.remove(pos);
                            }
                        }
                        // Delete
                        KeyEvent {
                            code: KeyCode::Delete,
                            ..
                        } => {
                            let pos = app.cursor;
                            if pos < app.input.len() {
                                app.input.remove(pos);
                            }
                        }
                        // Left/Right arrows
                        KeyEvent {
                            code: KeyCode::Left,
                            ..
                        } => {
                            if app.cursor > 0 {
                                app.cursor -= 1;
                            }
                        }
                        KeyEvent {
                            code: KeyCode::Right,
                            ..
                        } => {
                            if app.cursor < app.input.len() {
                                app.cursor += 1;
                            }
                        }
                        // Home/End
                        KeyEvent {
                            code: KeyCode::Home,
                            ..
                        } => app.cursor = 0,
                        KeyEvent {
                            code: KeyCode::End,
                            ..
                        } => app.cursor = app.input.len(),
                        // Up/Down: input history
                        KeyEvent {
                            code: KeyCode::Up, ..
                        } => {
                            if !app.history.is_empty() {
                                let idx = match app.history_index {
                                    Some(i) if i > 0 => i - 1,
                                    Some(i) => i,
                                    None => app.history.len() - 1,
                                };
                                app.history_index = Some(idx);
                                app.input = app.history[idx].clone();
                                app.cursor = app.input.len();
                            }
                        }
                        KeyEvent {
                            code: KeyCode::Down,
                            ..
                        } => {
                            if let Some(idx) = app.history_index {
                                if idx + 1 < app.history.len() {
                                    app.history_index = Some(idx + 1);
                                    app.input = app.history[idx + 1].clone();
                                } else {
                                    app.history_index = None;
                                    app.input.clear();
                                }
                                app.cursor = app.input.len();
                            }
                        }
                        // Scroll
                        KeyEvent {
                            code: KeyCode::PageUp,
                            ..
                        } => {
                            app.scroll = app.scroll.saturating_sub(10);
                            app.following = false;
                        }
                        KeyEvent {
                            code: KeyCode::PageDown,
                            ..
                        } => {
                            app.scroll += 10;
                            app.following = true;
                        }
                        // Ctrl+J / Shift+Enter: newline in input
                        KeyEvent {
                            code: KeyCode::Char('j'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }
                        | KeyEvent {
                            code: KeyCode::Enter,
                            modifiers: KeyModifiers::SHIFT,
                            ..
                        } => {
                            let pos = app.cursor;
                            app.input.insert(pos, '\n');
                            app.cursor += 1;
                        }
                        // Regular character
                        KeyEvent {
                            code: KeyCode::Char(c),
                            modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                            ..
                        } => {
                            let pos = app.cursor;
                            app.input.insert(pos, c);
                            app.cursor += 1;
                        }
                        _ => {}
                    }
                }
                Event::Paste(text) => {
                    let mut app = app.lock().await;
                    let pos = app.cursor;
                    app.input.insert_str(pos, &text);
                    app.cursor += text.len();
                }
                Event::Resize(_, _) => {
                    // Terminal will re-draw on next iteration
                }
                _ => {}
            }
        }

        // Process agent events (non-blocking)
        while let Ok(event) = ui_rx.0.try_recv() {
            let mut app = app.lock().await;
            match event {
                UiEvent::Token { content, .. } => {
                    // Append to the last assistant text or create a new one
                    if let Some(ChatLine::AssistantText(ref mut text)) = app.chat.last_mut() {
                        text.push_str(&content);
                    } else {
                        app.chat.push(ChatLine::AssistantText(content));
                    }
                    if app.following {
                        let h = terminal.size()?.height.saturating_sub(6);
                        app.scroll_to_bottom(h);
                    }
                }
                UiEvent::ToolCall { tool, input, .. } => {
                    // Extract a readable summary from tool input
                    let detail = match tool.as_str() {
                        "read_file" | "write_file" | "edit_file" | "list_files" => {
                            input.get("path").and_then(|v| v.as_str())
                                .unwrap_or("").to_string()
                        }
                        "shell" => {
                            input.get("command").and_then(|v| v.as_str())
                                .unwrap_or("").to_string()
                        }
                        "glob" => {
                            input.get("pattern").and_then(|v| v.as_str())
                                .unwrap_or("").to_string()
                        }
                        "grep" => {
                            input.get("pattern").and_then(|v| v.as_str())
                                .unwrap_or("").to_string()
                        }
                        _ => String::new(),
                    };
                    let label = if detail.is_empty() {
                        tool.clone()
                    } else {
                        format!("{tool} {detail}")
                    };
                    app.chat.push(ChatLine::ToolCall(label));
                    app.status = format!("tool:{tool}");
                }
                UiEvent::ToolResult {
                    output, is_error, ..
                } => {
                    let line_count = output.lines().count();
                    let short = if is_error {
                        output.lines().next().unwrap_or("error").to_string()
                    } else if line_count > 1 {
                        format!(
                            "{} ({} lines)",
                            output.lines().next().unwrap_or(""),
                            line_count
                        )
                    } else if output.len() > 100 {
                        format!("{}...", &output[..100])
                    } else {
                        output
                    };
                    app.chat.push(ChatLine::ToolResult(short, is_error));
                    app.status = "streaming".into();
                }
                UiEvent::Done { input_tokens, output_tokens, .. } => {
                    app.total_input_tokens += input_tokens;
                    app.total_output_tokens += output_tokens;
                    app.chat.push(ChatLine::System(format!(
                        "tokens: {}in / {}out",
                        format_tokens(input_tokens),
                        format_tokens(output_tokens),
                    )));
                    app.chat.push(ChatLine::Separator);
                    app.current_task = None;
                    app.status = "idle".into();
                }
                UiEvent::Error { message, .. } => {
                    app.chat.push(ChatLine::Error(message));
                    app.chat.push(ChatLine::Separator);
                    app.current_task = None;
                    app.status = "idle".into();
                }
                UiEvent::Status { message, .. } => {
                    app.chat.push(ChatLine::Status(message.clone()));
                    app.status = "retry".into();
                }
                UiEvent::PermissionRequest {
                    request_id,
                    tool,
                    resource,
                    ..
                } => {
                    app.pending_permission =
                        Some((request_id, tool.clone(), resource.clone()));
                    app.status = "permission?".into();
                }
            }
        }
    }
}

fn git_branch() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

fn format_tokens(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{n}")
    }
}

/// Draw the UI.
fn draw(f: &mut ratatui::Frame, app: &App) {
    // Input area grows with content (min 3, max 10)
    let input_lines = app.input.lines().count().max(1) as u16 + 2; // +2 for border
    let input_height = input_lines.clamp(3, 10);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),            // Chat history
            Constraint::Length(1),          // Status bar
            Constraint::Length(input_height), // Input
        ])
        .split(f.area());

    // Chat history
    let chat_lines: Vec<Line> = app
        .chat
        .iter()
        .flat_map(|line| match line {
            ChatLine::UserHeader => vec![Line::from(Span::styled(
                "You:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))],
            ChatLine::UserText(text) => text
                .lines()
                .map(|l| Line::from(l.to_string()))
                .collect(),
            ChatLine::AssistantHeader => vec![Line::from(Span::styled(
                "hob:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))],
            ChatLine::AssistantText(text) => text
                .lines()
                .map(|l| Line::from(l.to_string()))
                .collect(),
            ChatLine::ToolCall(tool) => vec![Line::from(Span::styled(
                format!("  ● {tool}"),
                Style::default().fg(Color::Yellow),
            ))],
            ChatLine::ToolResult(output, is_error) => {
                let color = if *is_error { Color::Red } else { Color::DarkGray };
                vec![Line::from(Span::styled(
                    format!("  ✓ {output}"),
                    Style::default().fg(color),
                ))]
            }
            ChatLine::Status(msg) => vec![Line::from(Span::styled(
                format!("  ⟳ {msg}"),
                Style::default().fg(Color::Yellow),
            ))],
            ChatLine::Error(msg) => vec![Line::from(Span::styled(
                format!("  ✗ {msg}"),
                Style::default().fg(Color::Red),
            ))],
            ChatLine::System(msg) => msg
                .lines()
                .map(|l| {
                    Line::from(Span::styled(
                        format!("  {l}"),
                        Style::default().fg(Color::Blue),
                    ))
                })
                .collect(),
            ChatLine::Separator => vec![Line::from(Span::styled(
                "─".repeat(f.area().width as usize),
                Style::default().fg(Color::DarkGray),
            ))],
        })
        .collect();

    let chat = Paragraph::new(Text::from(chat_lines))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0))
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(chat, chunks[0]);

    // Status bar
    let status_text = if let Some((_, ref tool, ref resource)) = app.pending_permission {
        format!(
            " ⚠ {tool}: {resource}  [y]once [!]always [n]reject ",
        )
    } else {
        let tokens = if app.total_input_tokens > 0 {
            format!(
                "  {}in/{}out",
                format_tokens(app.total_input_tokens),
                format_tokens(app.total_output_tokens),
            )
        } else {
            String::new()
        };
        let branch = git_branch().map(|b| format!("  git:{b}")).unwrap_or_default();
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| {
                let s = p.display().to_string();
                // Shorten home dir to ~
                if let Ok(home) = std::env::var("HOME") {
                    if let Some(rest) = s.strip_prefix(&home) {
                        return Some(format!("~{rest}"));
                    }
                }
                Some(s)
            })
            .unwrap_or_default();
        format!(" {cwd}  hob:{}{}{branch}  model:{}  /help ", app.status, tokens, app.model)
    };
    let status_style = match app.status.as_str() {
        "idle" => Style::default().fg(Color::DarkGray).bg(Color::Black),
        "streaming" => Style::default().fg(Color::Green).bg(Color::Black),
        "permission?" => Style::default().fg(Color::Yellow).bg(Color::Black),
        _ => Style::default().fg(Color::Yellow).bg(Color::Black),
    };
    let status = Paragraph::new(status_text).style(status_style);
    f.render_widget(status, chunks[1]);

    // Input area
    let input_title = if app.current_task.is_some() {
        " (streaming...) "
    } else {
        " > "
    };
    let input = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(input_title),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(input, chunks[2]);

    // Place cursor
    f.set_cursor_position((
        chunks[2].x + app.cursor as u16 + 1,
        chunks[2].y + 1,
    ));
}
