mod commands;
mod error;
mod state;
mod ui;

use arboard::Clipboard;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind},
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures::FutureExt;
use miette::{IntoDiagnostic, Result};
use ratatui::prelude::*;
use smol::channel::{bounded, Receiver};
use std::{
    fs::File,
    io::Write,
    time::{Duration, SystemTime},
};
use surf::Client;
use time::OffsetDateTime;

use crate::{
    commands::AVAILABLE_COMMANDS,
    error::{AppError, AppResult},
    state::{AppState, InputMode, RequestHistory},
    ui::render,
};

const MAX_HISTORY: usize = 100;

enum AppEvent {
    Input(CEvent),
    Tick,
}

struct App {
    state: AppState,
    events: Receiver<AppEvent>,
    client: Client,
    clipboard: Clipboard,
}

#[derive(Debug, serde::Deserialize)]
struct AuthResponse {
    #[serde(rename = "accessJwt")]
    access_jwt: String,
    #[serde(rename = "refreshJwt")]
    refresh_jwt: String,
}

struct TerminalHandler {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl TerminalHandler {
    fn new() -> AppResult<Self> {
        crossterm::terminal::enable_raw_mode().map_err(|e| AppError::Terminal {
            src: "terminal setup".into(),
            err_span: (0, 0),
            msg: e.to_string(),
        })?;

        std::io::stdout()
            .execute(EnterAlternateScreen)
            .map_err(|e| AppError::Terminal {
                src: "terminal setup".into(),
                err_span: (0, 0),
                msg: e.to_string(),
            })?
            .execute(EnableMouseCapture)
            .map_err(|e| AppError::Terminal {
                src: "terminal setup".into(),
                err_span: (0, 0),
                msg: e.to_string(),
            })?;

        let backend = CrosstermBackend::new(std::io::stdout());
        let terminal = Terminal::new(backend).map_err(|e| AppError::Terminal {
            src: "terminal creation".into(),
            err_span: (0, 0),
            msg: e.to_string(),
        })?;

        Ok(Self { terminal })
    }
}

impl Drop for TerminalHandler {
    fn drop(&mut self) {
        crossterm::terminal::disable_raw_mode().ok();
        std::io::stdout().execute(LeaveAlternateScreen).ok();
        std::io::stdout().execute(DisableMouseCapture).ok();
    }
}

impl App {
    fn new() -> Result<Self> {
        let (tx, rx) = bounded(100);

        let event_tx = tx.clone();
        smol::spawn(async move {
            loop {
                if event::poll(Duration::from_millis(100)).unwrap() {
                    if let Ok(event) = event::read() {
                        let _ = event_tx.send(AppEvent::Input(event)).await;
                    }
                }
                let _ = event_tx.send(AppEvent::Tick).await;
            }
        })
        .detach();

        let client = surf::Config::new()
            .set_timeout(Some(Duration::from_secs(10)))
            .try_into()
            .into_diagnostic()?;

        Ok(Self {
            state: AppState::default(),
            events: rx,
            client,
            clipboard: Clipboard::new().into_diagnostic()?,
        })
    }

    async fn handle_input(&mut self, event: CEvent) -> AppResult<()> {
        if let CEvent::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

            if key.modifiers.contains(event::KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('c')
            {
                self.state.quit = true;
                return Ok(());
            }

            let current_mode = self.state.input.mode.clone();
            match current_mode {
                InputMode::Normal => match key.code {
                    KeyCode::Enter => {
                        if !self.state.input.content.is_empty() {
                            let identifier = self.state.input.content.clone();
                            self.state.input.content.clear();
                            self.state.input.mode = InputMode::Password;
                            self.state.input.cursor_position = 0;
                            self.state.identifier = Some(identifier);
                        }
                    }
                    _ => {
                        self.state.input.handle_key(key.code);
                    }
                },
                InputMode::Password => match key.code {
                    KeyCode::Enter => {
                        if let Some(identifier) = self.state.identifier.take() {
                            let password = self.state.input.content.clone();
                            self.state.input.content.clear();
                            self.state.input.cursor_position = 0;

                            match self.handle_auth(identifier.clone(), password).await {
                                Ok(()) => {
                                    self.state.input.mode = InputMode::Command;
                                }
                                Err(e) => {
                                    self.state.error =
                                        Some(format!("Authentication failed: {}", e));
                                    self.state.error_time = Some(SystemTime::now());
                                    self.state.input.mode = InputMode::Normal;
                                }
                            }
                        }
                    }
                    _ => {
                        self.state.input.handle_key(key.code);
                    }
                },
                InputMode::Command => match key.code {
                    KeyCode::Enter => {
                        let command = if !self.state.input.content.is_empty() {
                            self.state.input.content.clone()
                        } else if let Some(idx) = self.state.selected_command_index {
                            AVAILABLE_COMMANDS[idx].method.to_string()
                        } else {
                            return Ok(());
                        };

                        if let Some(cmd) = AVAILABLE_COMMANDS.iter().find(|c| c.method == command) {
                            self.state.input.content.clear();
                            self.state.input.cursor_position = 0;
                            self.state.output = None;

                            self.state.input.mode = InputMode::CommandBuilder {
                                command: cmd.method.to_string(),
                                current_param: 0,
                                params: Vec::new(),
                            };
                        }
                    }
                    KeyCode::Up => {
                        if let Some(idx) = self.state.selected_command_index {
                            if idx > 0 {
                                self.state.selected_command_index = Some(idx - 1);
                            }
                        } else {
                            self.state.selected_command_index = Some(AVAILABLE_COMMANDS.len() - 1);
                        }
                    }
                    KeyCode::Down => {
                        if let Some(idx) = self.state.selected_command_index {
                            if idx < AVAILABLE_COMMANDS.len() - 1 {
                                self.state.selected_command_index = Some(idx + 1);
                            }
                        } else {
                            self.state.selected_command_index = Some(0);
                        }
                    }
                    KeyCode::Tab => {
                        if let Some(idx) = self.state.input.completion_index {
                            if let Some(completion) = self.state.input.completion_matches.get(idx) {
                                self.state.input.content = completion.clone();
                                self.state.input.cursor_position = self.state.input.content.len();
                                self.state.input.completion_index =
                                    Some((idx + 1) % self.state.input.completion_matches.len());
                            }
                        } else if !self.state.input.content.is_empty() {
                            self.state.input.update_completions();
                        }
                    }
                    KeyCode::Char('h') | KeyCode::Char('H') => {
                        self.state.input.mode = InputMode::History;
                        self.state.selected_command_index =
                            if !self.state.request_history.is_empty() {
                                Some(0)
                            } else {
                                None
                            };
                    }
                    _ => {
                        self.state.input.handle_key(key.code);
                        if !self.state.input.content.is_empty() {
                            self.state.input.update_completions();
                        }
                    }
                },
                InputMode::History => match key.code {
                    KeyCode::Enter => {
                        if let Some(idx) = self.state.selected_command_index {
                            if let Some(hist) = self.state.request_history.get(idx) {
                                let method = hist.method.clone();
                                let params = hist.params.clone();
                                self.execute_command(&method, &params).await?;
                                self.state.input.mode = InputMode::ViewingResponse;
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.state.input.mode = InputMode::Command;
                        self.state.selected_command_index = Some(0);
                    }
                    KeyCode::Up => {
                        if let Some(idx) = self.state.selected_command_index {
                            if idx > 0 {
                                self.state.selected_command_index = Some(idx - 1);
                            }
                        }
                    }
                    KeyCode::Down => {
                        if let Some(idx) = self.state.selected_command_index {
                            if idx < self.state.request_history.len() - 1 {
                                self.state.selected_command_index = Some(idx + 1);
                            }
                        }
                    }
                    _ => {}
                },
                InputMode::CommandBuilder {
                    command,
                    current_param,
                    params,
                } => match key.code {
                    KeyCode::Enter => {
                        let cmd = AVAILABLE_COMMANDS
                            .iter()
                            .find(|c| c.method == command)
                            .ok_or_else(|| AppError::Request {
                                src: "command validation".into(),
                                err_span: (0, 0),
                                msg: "Command not found".into(),
                            })?;

                        let param = &cmd.parameters[current_param];
                        let mut new_params = params.clone();

                        let param_value = if self.state.input.content.is_empty() {
                            if param.optional {
                                param.default.unwrap_or("").to_string()
                            } else {
                                return Ok(());
                            }
                        } else {
                            self.state.input.content.clone()
                        };

                        if new_params.len() == current_param {
                            new_params.push(param_value);
                        } else {
                            new_params[current_param] = param_value;
                        }

                        self.state.input.content.clear();
                        self.state.input.cursor_position = 0;

                        if current_param + 1 < cmd.parameters.len() {
                            self.state.input.mode = InputMode::CommandBuilder {
                                command,
                                current_param: current_param + 1,
                                params: new_params,
                            };
                        } else {
                            self.execute_command(&command, &new_params).await?;
                            self.state.input.mode = InputMode::ViewingResponse;
                        }
                    }
                    KeyCode::Esc => {
                        self.state.input.content.clear();
                        self.state.input.cursor_position = 0;
                        self.state.input.mode = InputMode::Command;
                    }
                    _ => {
                        self.state.input.handle_key(key.code);
                    }
                },
                InputMode::ViewingResponse => {
                    let viewport_height = if let Ok((_, rows)) = crossterm::terminal::size() {
                        // Subtract 7 for the header (3), status (3), and help (1) areas
                        rows.saturating_sub(7)
                    } else {
                        0
                    };

                    match key.code {
                        KeyCode::Enter => {
                            self.state.input.mode = InputMode::Command;
                            self.state.input.content.clear();
                            self.state.input.cursor_position = 0;
                            self.state.scroll_offset = 0; // Reset scroll position
                        }
                        KeyCode::Up => {
                            self.update_scroll(-1, viewport_height);
                        }
                        KeyCode::Down => {
                            self.update_scroll(1, viewport_height);
                        }
                        KeyCode::PageUp => {
                            self.update_scroll(-10, viewport_height);
                        }
                        KeyCode::PageDown => {
                            self.update_scroll(10, viewport_height);
                        }
                        KeyCode::Home => {
                            self.state.scroll_offset = 0;
                        }
                        KeyCode::End => {
                            let max_scroll =
                                self.get_content_height().saturating_sub(viewport_height);
                            self.state.scroll_offset = max_scroll;
                        }
                        KeyCode::Char('c') => {
                            if let Some(output) = &self.state.output {
                                match serde_json::to_string_pretty(output) {
                                    Ok(json_str) => {
                                        if let Err(e) = self.clipboard.set_text(json_str) {
                                            self.state.error =
                                                Some(format!("Failed to copy to clipboard: {}", e));
                                            self.state.error_time = Some(SystemTime::now());
                                        }
                                    }
                                    Err(e) => {
                                        self.state.error =
                                            Some(format!("Failed to format JSON: {}", e));
                                        self.state.error_time = Some(SystemTime::now());
                                    }
                                }
                            }
                        }
                        KeyCode::Char('e') => {
                            if let Some(output) = &self.state.output {
                                let now = OffsetDateTime::now_utc();
                                let filename = format!(
                                    "bsky_response_{:04}_{:02}_{:02}_{:02}_{:02}_{:02}.json",
                                    now.year(),
                                    now.month() as u8,
                                    now.day(),
                                    now.hour(),
                                    now.minute(),
                                    now.second()
                                );

                                match serde_json::to_string_pretty(output) {
                                    Ok(json_str) => match File::create(&filename) {
                                        Ok(mut file) => match file.write_all(json_str.as_bytes()) {
                                            Ok(_) => {
                                                self.state.error =
                                                    Some(format!("Exported to {}", filename));
                                                self.state.error_time = Some(SystemTime::now());
                                            }
                                            Err(e) => {
                                                self.state.error =
                                                    Some(format!("Failed to write file: {}", e));
                                                self.state.error_time = Some(SystemTime::now());
                                            }
                                        },
                                        Err(e) => {
                                            self.state.error =
                                                Some(format!("Failed to write file: {}", e));
                                            self.state.error_time = Some(SystemTime::now());
                                        }
                                    },
                                    Err(e) => {
                                        self.state.error =
                                            Some(format!("Failed to format JSON: {}", e));
                                        self.state.error_time = Some(SystemTime::now());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_auth(&mut self, identifier: String, password: String) -> AppResult<()> {
        self.state.error = None;

        let json_body = serde_json::json!({
            "identifier": identifier,
            "password": password
        });

        let endpoint = format!(
            "{}/xrpc/com.atproto.server.createSession",
            self.state.pds_host.trim_end_matches('/')
        );

        let mut res = match self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .body_json(&json_body)
            .map_err(|e| AppError::Auth {
                src: "building auth request".into(),
                err_span: (0, 0),
                msg: format!("Failed to build auth request: {}", e),
            })?
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let error_msg = format!("Auth request failed: {}", e);
                self.state.error = Some(error_msg.clone());
                return Err(AppError::Auth {
                    src: "authentication".into(),
                    err_span: (0, 0),
                    msg: error_msg,
                }
                .into());
            }
        };

        if !res.status().is_success() {
            let status = res.status();
            let error_body = match res.body_string().await {
                Ok(text) => text,
                Err(e) => format!("Failed to read error response: {}", e),
            };

            let error_msg = format!("Auth failed ({}): {}", status, error_body);
            self.state.error = Some(error_msg.clone());
            self.state.error_time = Some(SystemTime::now());

            return Err(AppError::Auth {
                src: "authentication".into(),
                err_span: (0, 0),
                msg: error_msg,
            }
            .into());
        }

        let auth_response = match res.body_json::<AuthResponse>().await {
            Ok(resp) => resp,
            Err(e) => {
                return Err(AppError::Auth {
                    src: "parsing response".into(),
                    err_span: (0, 0),
                    msg: format!("Failed to parse response as JSON: {}", e),
                }
                .into());
            }
        };

        self.state.auth_token = Some(auth_response.access_jwt);
        self.state.refresh_token = Some(auth_response.refresh_jwt);
        self.state.is_authenticated = true;
        Ok(())
    }

    async fn execute_command(&mut self, method: &str, params: &[String]) -> AppResult<()> {
        let cmd = AVAILABLE_COMMANDS
            .iter()
            .find(|c| c.method == method)
            .ok_or_else(|| AppError::Request {
                src: "executing command".into(),
                err_span: (0, 0),
                msg: "Command not found".into(),
            })?;

        let mut url = format!(
            "{}/xrpc/{}",
            self.state.pds_host.trim_end_matches('/'),
            method
        );

        let mut query_params: Vec<(String, String)> = Vec::new();
        for (i, param) in cmd.parameters.iter().enumerate() {
            if let Some(value) = params.get(i) {
                if !value.is_empty() || !param.optional {
                    query_params.push((param.name.to_string(), value.clone()));
                }
            }
        }

        if !query_params.is_empty() {
            url.push('?');
            for (i, (name, value)) in query_params.iter().enumerate() {
                if i > 0 {
                    url.push('&');
                }
                url.push_str(&format!("{}={}", name, value));
            }
        }

        self.add_to_history(method, url.clone(), params.to_vec());

        let mut req = self.client.get(&url);
        if let Some(token) = &self.state.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        match req.send().await {
            Ok(mut res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let error_body = match res.body_string().await {
                        Ok(text) => text,
                        Err(e) => format!("Failed to read error response: {}", e),
                    };

                    let error_msg = format!("Request failed ({}): {}", status, error_body);
                    self.state.error = Some(error_msg.clone());
                    self.update_history_success(method, false);
                    return Err(AppError::Request {
                        src: "request".into(),
                        err_span: (0, 0),
                        msg: error_msg,
                    }
                    .into());
                }

                match res.body_json::<serde_json::Value>().await {
                    Ok(json) => {
                        self.state.output = Some(json);
                        self.state.error = None;
                        self.update_history_success(method, true);
                        Ok(())
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to parse response: {}", e);
                        self.state.error = Some(error_msg.clone());
                        self.update_history_success(method, false);
                        Err(AppError::Request {
                            src: "parsing response".into(),
                            err_span: (0, 0),
                            msg: error_msg,
                        }
                        .into())
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Request failed: {}", e);
                self.state.error = Some(error_msg.clone());
                self.update_history_success(method, false);
                Err(AppError::Request {
                    src: "request".into(),
                    err_span: (0, 0),
                    msg: error_msg,
                }
                .into())
            }
        }
    }

    fn add_to_history(&mut self, method: &str, url: String, params: Vec<String>) {
        self.state.request_history.push_front(RequestHistory {
            method: method.to_string(),
            timestamp: OffsetDateTime::now_utc(),
            success: false,
            url,
            params,
        });

        if self.state.request_history.len() > MAX_HISTORY {
            self.state.request_history.pop_back();
        }
    }

    fn update_history_success(&mut self, method: &str, success: bool) {
        if let Some(hist) = self
            .state
            .request_history
            .iter_mut()
            .find(|h| h.method == method)
        {
            hist.success = success;
        }
    }

    fn get_content_height(&self) -> u16 {
        if let Some(output) = &self.state.output {
            let formatted = serde_json::to_string_pretty(output).unwrap_or_default();
            let text = ui::syntax_highlight(&formatted);
            text.lines.len() as u16
        } else if self.state.error.is_some() {
            1
        } else {
            0
        }
    }

    fn update_scroll(&mut self, direction: i16, viewport_height: u16) {
        let content_height = self.get_content_height();
        let max_scroll = content_height.saturating_sub(viewport_height);

        match direction {
            1 => {
                // Scroll down
                self.state.scroll_offset = (self.state.scroll_offset + 1).min(max_scroll);
            }
            -1 => {
                // Scroll up
                self.state.scroll_offset = self.state.scroll_offset.saturating_sub(1);
            }
            10 => {
                // Page down
                self.state.scroll_offset =
                    (self.state.scroll_offset + viewport_height).min(max_scroll);
            }
            -10 => {
                // Page up
                self.state.scroll_offset = self.state.scroll_offset.saturating_sub(viewport_height);
            }
            _ => {}
        }
    }

    async fn run(&mut self) -> AppResult<()> {
        let mut terminal_handler = TerminalHandler::new()?;

        while !self.state.quit {
            terminal_handler
                .terminal
                .draw(|f| render(&self.state, f))
                .map_err(|e| AppError::Terminal {
                    src: "drawing terminal".into(),
                    err_span: (0, 0),
                    msg: e.to_string(),
                })?;

            match self.events.try_recv() {
                Ok(AppEvent::Input(event)) => {
                    if let Err(e) = self.handle_input(event).await {
                        self.state.error = Some(e.to_string());
                        self.state.error_time = Some(SystemTime::now());
                    }
                }
                Ok(AppEvent::Tick) => {
                    self.state.update();
                }
                Err(smol::channel::TryRecvError::Empty) => {
                    smol::Timer::after(Duration::from_millis(10)).await;
                }
                Err(smol::channel::TryRecvError::Closed) => {
                    break;
                }
            }
        }

        Ok(())
    }

    // async fn refresh_session(&mut self) -> AppResult<()> {
    //     if let Some(refresh_token) = &self.state.refresh_token {
    //         let endpoint = format!(
    //             "{}/xrpc/com.atproto.server.refreshSession",
    //             self.state.pds_host.trim_end_matches('/')
    //         );

    //         let mut res = match self
    //             .client
    //             .post(&endpoint)
    //             .header("Authorization", format!("Bearer {}", refresh_token))
    //             .await
    //         {
    //             Ok(res) => res,
    //             Err(e) => {
    //                 let error_msg = format!("Failed to refresh session: {}", e);
    //                 self.state.error = Some(error_msg.clone());
    //                 return Err(AppError::Auth {
    //                     src: "session refresh".into(),
    //                     err_span: (0, 0),
    //                     msg: error_msg,
    //                 }
    //                 .into());
    //             }
    //         };

    //         if !res.status().is_success() {
    //             self.state.is_authenticated = false;
    //             self.state.auth_token = None;
    //             self.state.refresh_token = None;
    //             return Err(AppError::Auth {
    //                 src: "session refresh".into(),
    //                 err_span: (0, 0),
    //                 msg: "Session refresh failed".into(),
    //             }
    //             .into());
    //         }

    //         let auth_response = match res.body_json::<AuthResponse>().await {
    //             Ok(resp) => resp,
    //             Err(e) => {
    //                 return Err(AppError::Auth {
    //                     src: "parsing refresh response".into(),
    //                     err_span: (0, 0),
    //                     msg: format!("Failed to parse refresh response: {}", e),
    //                 }
    //                 .into());
    //             }
    //         };

    //         self.state.auth_token = Some(auth_response.access_jwt);
    //         self.state.refresh_token = Some(auth_response.refresh_jwt);
    //         Ok(())
    //     } else {
    //         Err(AppError::Auth {
    //             src: "session refresh".into(),
    //             err_span: (0, 0),
    //             msg: "No refresh token available".into(),
    //         }
    //         .into())
    //     }
    // }
}

fn main() -> AppResult<()> {
    #[cfg(debug_assertions)]
    std::env::set_var("RUST_BACKTRACE", "1");

    let result = smol::block_on(async {
        let app_result = std::panic::AssertUnwindSafe(App::new()?.run())
            .catch_unwind()
            .await;

        match app_result {
            Ok(res) => res,
            Err(err) => {
                let panic_msg = if let Some(s) = err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic occurred".to_string()
                };

                Err(AppError::Terminal {
                    src: "panic".into(),
                    err_span: (0, 0),
                    msg: panic_msg,
                }
                .into())
            }
        }
    });

    crossterm::terminal::disable_raw_mode().ok();
    std::io::stdout().execute(LeaveAlternateScreen).ok();
    std::io::stdout().execute(DisableMouseCapture).ok();

    if let Err(e) = &result {
        println!("Application error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
