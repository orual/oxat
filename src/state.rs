use crossterm::event::KeyCode;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    time::{Duration, SystemTime},
};
use time::OffsetDateTime;

use crate::commands::AVAILABLE_COMMANDS;

const MAX_HISTORY: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHistory {
    pub method: String,
    pub timestamp: OffsetDateTime,
    pub success: bool,
    pub url: String,
    pub params: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum InputMode {
    #[default]
    Normal,
    Password,
    Command,
    History,
    CommandBuilder {
        command: String,
        current_param: usize,
        params: Vec<String>,
    },
    ServiceAuth {
        command: String,
        current_param: usize,
        params: Vec<String>,
    },
    ViewingResponse,
}

#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub content: String,
    pub cursor_position: usize,
    pub mode: InputMode,
    pub completion_index: Option<usize>,
    pub completion_matches: Vec<String>,
}

impl InputState {
    pub fn update_completions(&mut self) {
        if let InputMode::Command = self.mode {
            if self.content.is_empty() {
                self.completion_matches.clear();
                self.completion_index = None;
                return;
            }

            self.completion_matches = AVAILABLE_COMMANDS
                .iter()
                .map(|cmd| cmd.method)
                .filter(|method| method.starts_with(&self.content))
                .map(|s| s.to_string())
                .collect();

            self.completion_index = if self.completion_matches.is_empty() {
                None
            } else if let Some(idx) = self.completion_index {
                if idx < self.completion_matches.len() {
                    Some(idx)
                } else {
                    Some(0)
                }
            } else {
                Some(0)
            };
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char(c) => {
                self.content.insert(self.cursor_position, c);
                self.cursor_position += 1;
                self.update_completions();
                true
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.content.remove(self.cursor_position - 1);
                    self.cursor_position -= 1;
                    self.update_completions();
                }
                true
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
                true
            }
            KeyCode::Right => {
                if self.cursor_position < self.content.len() {
                    self.cursor_position += 1;
                }
                true
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub input: InputState,
    pub auth_token: Option<String>,
    pub refresh_token: Option<String>,
    pub service_auth: Option<String>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub error_time: Option<SystemTime>,
    pub pds_host: String,
    pub is_authenticated: bool,
    pub request_history: VecDeque<RequestHistory>,
    pub quit: bool,
    pub identifier: Option<String>,
    pub selected_command_index: Option<usize>,
    pub scroll_offset: u16,
}

impl AppState {
    pub fn update(&mut self) {
        if let Some(error_time) = self.error_time {
            if error_time.elapsed().unwrap_or_default() >= Duration::from_secs(5) {
                self.error = None;
                self.error_time = None;
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            input: InputState::default(),
            auth_token: None,
            refresh_token: None,
            output: None,
            error: None,
            error_time: None,
            pds_host: "https://bsky.social".to_string(),
            is_authenticated: false,
            request_history: VecDeque::with_capacity(MAX_HISTORY),
            quit: false,
            identifier: None,
            selected_command_index: Some(0),
            scroll_offset: 0,
            service_auth: None,
        }
    }
}
