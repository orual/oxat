mod error;
mod state;
mod ui;

use crate::{
    error::{AppError, AppResult},
    state::{AppState, InputMode, RequestHistory},
    ui::render,
};

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
use state::AVAILABLE_COMMANDS;
use std::time::{Duration, SystemTime};
use surf::Client;

const MAX_HISTORY: usize = 100;

enum AppEvent {
    Input(CEvent),
    Tick,
}

struct App {
    state: AppState,
    events: Receiver<AppEvent>,
    client: Client,
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
                        if self.state.input.content.is_empty() {
                            if let Some(idx) = self.state.selected_command_index {
                                if idx > 0 {
                                    self.state.selected_command_index = Some(idx - 1);
                                }
                            } else {
                                self.state.selected_command_index =
                                    Some(AVAILABLE_COMMANDS.len() - 1);
                            }
                        } else if let Some(idx) = self.state.history_index.map(|i| i + 1) {
                            if idx < self.state.request_history.len() {
                                if let Some(hist) = self.state.request_history.get(idx) {
                                    self.state.input.content = hist.method.clone();
                                    self.state.input.cursor_position = hist.method.len();
                                    self.state.history_index = Some(idx);
                                }
                            }
                        } else if !self.state.request_history.is_empty() {
                            let hist = &self.state.request_history[0];
                            self.state.input.content = hist.method.clone();
                            self.state.input.cursor_position = hist.method.len();
                            self.state.history_index = Some(0);
                        }
                    }
                    KeyCode::Down => {
                        if self.state.input.content.is_empty() {
                            if let Some(idx) = self.state.selected_command_index {
                                if idx < AVAILABLE_COMMANDS.len() - 1 {
                                    self.state.selected_command_index = Some(idx + 1);
                                }
                            } else {
                                self.state.selected_command_index = Some(0);
                            }
                        } else if let Some(idx) = self.state.history_index {
                            if idx > 0 {
                                if let Some(hist) = self.state.request_history.get(idx - 1) {
                                    self.state.input.content = hist.method.clone();
                                    self.state.input.cursor_position = hist.method.len();
                                    self.state.history_index = Some(idx - 1);
                                }
                            } else {
                                self.state.input.content.clear();
                                self.state.input.cursor_position = 0;
                                self.state.history_index = None;
                            }
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
                    _ => {
                        self.state.input.handle_key(key.code);
                    }
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
                    if key.code == KeyCode::Enter {
                        self.state.input.content.clear();
                        self.state.input.cursor_position = 0;
                        self.state.input.mode = InputMode::Command;
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

        self.add_to_history(method);

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

    fn add_to_history(&mut self, method: &str) {
        self.state.request_history.push_front(RequestHistory {
            method: method.to_string(),
            timestamp: SystemTime::now(),
            success: false,
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
