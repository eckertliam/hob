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
}

impl App {
    fn new() -> Self {
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
        }
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
        // Rough estimate — each ChatLine is at least one terminal line
        self.chat.len()
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
    execute!(stdout, EnterAlternateScreen)?;
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
    )
    .await;

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
) -> anyhow::Result<()> {
    let app = Arc::new(Mutex::new(App::new()));

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
                        // Ctrl-C / Ctrl-D: quit
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
                                // Cancel current task
                                if let Some(ref id) = app.current_task {
                                    action_tx
                                        .send(UserAction::Cancel { id: id.clone() })
                                        .await;
                                }
                            } else {
                                return Ok(());
                            }
                        }
                        // Enter: send input
                        KeyEvent {
                            code: KeyCode::Enter,
                            modifiers: KeyModifiers::NONE,
                            ..
                        } => {
                            if !app.input.trim().is_empty() {
                                let prompt = app.input.trim().to_string();
                                app.history.push(prompt.clone());
                                app.history_index = None;
                                app.input.clear();
                                app.cursor = 0;

                                let id = app.next_task_id();
                                app.current_task = Some(id.clone());
                                app.status = "streaming".into();

                                app.chat.push(ChatLine::Separator);
                                app.chat.push(ChatLine::UserHeader);
                                app.chat.push(ChatLine::UserText(prompt.clone()));
                                app.chat.push(ChatLine::AssistantHeader);
                                app.following = true;

                                action_tx
                                    .send(UserAction::Task { id, prompt })
                                    .await;
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
                UiEvent::ToolCall { tool, .. } => {
                    app.chat.push(ChatLine::ToolCall(tool.clone()));
                    app.status = format!("tool:{tool}");
                }
                UiEvent::ToolResult {
                    output, is_error, ..
                } => {
                    let short = if output.len() > 100 {
                        format!("{}...", &output[..100])
                    } else {
                        output
                    };
                    app.chat.push(ChatLine::ToolResult(short, is_error));
                    app.status = "streaming".into();
                }
                UiEvent::Done { .. } => {
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

/// Draw the UI.
fn draw(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),        // Chat history
            Constraint::Length(1),      // Status bar
            Constraint::Length(3),      // Input
        ])
        .split(f.area());

    // Chat history
    let chat_lines: Vec<Line> = app
        .chat
        .iter()
        .map(|line| match line {
            ChatLine::UserHeader => Line::from(Span::styled(
                "You:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            ChatLine::UserText(text) => Line::from(text.as_str()),
            ChatLine::AssistantHeader => Line::from(Span::styled(
                "hob:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            ChatLine::AssistantText(text) => Line::from(text.as_str()),
            ChatLine::ToolCall(tool) => Line::from(Span::styled(
                format!("  ● {tool}"),
                Style::default().fg(Color::Yellow),
            )),
            ChatLine::ToolResult(output, is_error) => {
                let color = if *is_error { Color::Red } else { Color::DarkGray };
                Line::from(Span::styled(
                    format!("  ✓ {output}"),
                    Style::default().fg(color),
                ))
            }
            ChatLine::Status(msg) => Line::from(Span::styled(
                format!("  ⟳ {msg}"),
                Style::default().fg(Color::Yellow),
            )),
            ChatLine::Error(msg) => Line::from(Span::styled(
                format!("  ✗ {msg}"),
                Style::default().fg(Color::Red),
            )),
            ChatLine::Separator => Line::from(Span::styled(
                "─".repeat(f.area().width as usize),
                Style::default().fg(Color::DarkGray),
            )),
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
        format!(" hob:{} ", app.status)
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
