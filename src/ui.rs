use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Position,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::state::{AppState, InputMode, AVAILABLE_COMMANDS};

pub fn render(app: &AppState, f: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input
            Constraint::Length(3), // Status
            Constraint::Min(0),    // Main content
            Constraint::Length(1), // Help
        ])
        .split(f.area());

    render_input(app, f, chunks[0]);
    render_status(app, f, chunks[1]);

    match &app.input.mode {
        InputMode::Command => {
            render_commands(app, f, chunks[2]);
        }
        InputMode::History => {
            render_history(app, f, chunks[2]);
        }
        InputMode::CommandBuilder { .. } => {
            render_command_builder(app, f, chunks[2]);
        }
        InputMode::ViewingResponse => {
            render_output(app, f, chunks[2]);
        }
        _ => {
            render_output(app, f, chunks[2]);
        }
    }

    render_help(app, f, chunks[3]);
}

fn render_input(app: &AppState, f: &mut Frame, area: Rect) {
    let input_style = match app.input.mode {
        InputMode::Password => Style::default().fg(Color::Red),
        InputMode::Command => Style::default().fg(Color::Yellow),
        InputMode::CommandBuilder { .. } => Style::default().fg(Color::Green),
        InputMode::ViewingResponse => Style::default().fg(Color::Blue),
        InputMode::History => Style::default().fg(Color::Yellow),
        InputMode::Normal => Style::default(),
    };

    let title = match &app.input.mode {
        InputMode::Password => "Enter your password",
        InputMode::Normal => "Enter your identifier",
        InputMode::Command => "Enter or select a command (Tab to autocomplete)",
        InputMode::History => "Command History",
        InputMode::CommandBuilder {
            command,
            current_param,
            ..
        } => {
            &if let Some(cmd) = AVAILABLE_COMMANDS.iter().find(|c| c.method == *command) {
                if let Some(param) = cmd.parameters.get(*current_param) {
                    if param.optional {
                        format!(
                            "Enter {} (optional, default: {})",
                            param.name,
                            param.default.unwrap_or("none")
                        )
                    } else {
                        format!("Enter {}", param.name)
                    }
                } else {
                    "Enter parameter".to_string()
                }
            } else {
                "Enter parameter".to_string()
            }
        }
        InputMode::ViewingResponse => "Press Enter to return to command list",
    };

    let input_content = if app.input.mode == InputMode::Password {
        "•".repeat(app.input.content.len())
    } else {
        app.input.content.clone()
    };

    let mut text = Text::from(input_content);
    text = text.patch_style(input_style);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Cyan));

    f.render_widget(input_block.clone(), area);
    let inner_area = input_block.inner(area);

    let input = Paragraph::new(text);
    f.render_widget(input, inner_area);

    // Render autocompletion
    if let InputMode::Command = app.input.mode {
        let mut spans = Vec::new();

        spans.push(Span::styled(app.input.content.clone(), input_style));

        if !app.input.content.is_empty() {
            if let Some(idx) = app.input.completion_index {
                if let Some(completion) = app.input.completion_matches.get(idx) {
                    if let Some(suggestion) = completion.strip_prefix(&app.input.content) {
                        spans.push(Span::styled(
                            suggestion,
                            Style::default().fg(Color::DarkGray),
                        ));

                        spans.push(Span::styled(
                            format!(" ({}/{})", idx + 1, app.input.completion_matches.len()),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
            }
        }

        let text = Text::from(Line::from(spans));
        let input = Paragraph::new(text);
        f.render_widget(input, inner_area);
    } else {
        let text = Text::from(if app.input.mode == InputMode::Password {
            "•".repeat(app.input.content.len())
        } else {
            app.input.content.clone()
        })
        .patch_style(input_style);

        let input = Paragraph::new(text);
        f.render_widget(input, inner_area);
    }

    f.set_cursor_position(Position {
        x: area.x + 1 + app.input.cursor_position as u16,
        y: area.y + 1,
    });
}

fn render_status(app: &AppState, f: &mut Frame, area: Rect) {
    let status = if app.is_authenticated {
        vec![
            Span::raw("Authenticated | "),
            Span::styled("PDS: ", Style::default().fg(Color::Gray)),
            Span::styled(&app.pds_host, Style::default().fg(Color::Green)),
        ]
    } else {
        vec![Span::styled(
            "Not authenticated",
            Style::default().fg(Color::Red),
        )]
    };

    let status = Paragraph::new(Line::from(status))
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    f.render_widget(status, area);
}

fn render_commands(app: &AppState, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .title("Available Commands")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = AVAILABLE_COMMANDS
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let style = if Some(i) == app.selected_command_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
            };

            let header_line = Line::from(vec![Span::styled(cmd.method, style)]);

            let desc_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(cmd.description, Style::default().fg(Color::Gray)),
            ]);

            let mut lines = vec![header_line, desc_line];

            for param in cmd.parameters {
                let param_desc = if param.optional {
                    format!(
                        "{} (optional, default: {})",
                        param.description,
                        param.default.unwrap_or("none")
                    )
                } else {
                    param.description.to_string()
                };

                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(param.name, Style::default().fg(Color::Cyan)),
                    Span::raw(": "),
                    Span::styled(param_desc, Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items).block(Block::default()).highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    f.render_widget(list, inner);
}

fn render_history(app: &AppState, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .title("Command History")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = app
        .request_history
        .iter()
        .enumerate()
        .map(|(i, hist)| {
            let style = if Some(i) == app.selected_command_index {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
            };

            let time_str = format!(
                "{:02}:{:02}:{:02}",
                hist.timestamp.hour(),
                hist.timestamp.minute(),
                hist.timestamp.second()
            );

            let status_style = if hist.success {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };

            let header_line = Line::from(vec![
                Span::styled(time_str, Style::default().fg(Color::Gray)),
                Span::raw(" "),
                Span::styled(if hist.success { "✓" } else { "✗" }, status_style),
                Span::raw(" "),
                Span::styled(&hist.method, style),
            ]);

            let url_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(&hist.url, Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(vec![header_line, url_line])
        })
        .collect();

    let list = List::new(items).block(Block::default()).highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    f.render_widget(list, inner);
}

fn render_command_builder(app: &AppState, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .title("Command Builder")
        .borders(Borders::ALL);
    let inner = block.inner(area);

    if let InputMode::CommandBuilder {
        command,
        current_param,
        params,
    } = &app.input.mode
    {
        if let Some(cmd) = AVAILABLE_COMMANDS.iter().find(|c| c.method == *command) {
            let mut text = vec![
                Line::from(vec![
                    Span::raw("Building command: "),
                    Span::styled(
                        cmd.method,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
            ];

            for (i, param) in cmd.parameters.iter().enumerate() {
                let value = params.get(i).map(|s| s.as_str()).unwrap_or("");
                let style = match i.cmp(current_param) {
                    std::cmp::Ordering::Equal => Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    std::cmp::Ordering::Less => Style::default().fg(Color::Gray),
                    std::cmp::Ordering::Greater => Style::default().fg(Color::DarkGray),
                };

                let param_text = if param.optional {
                    format!("{} (optional): ", param.name)
                } else {
                    format!("{}: ", param.name)
                };

                text.push(Line::from(vec![
                    Span::styled(param_text, style),
                    Span::styled(value, style),
                ]));

                let desc = if param.optional {
                    format!(
                        "{} (default: {})",
                        param.description,
                        param.default.unwrap_or("none")
                    )
                } else {
                    param.description.to_string()
                };

                text.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(desc, Style::default().fg(Color::DarkGray)),
                ]));
            }

            let paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
            f.render_widget(paragraph, inner);
        }
    }
}

fn render_output(app: &AppState, f: &mut Frame, area: Rect) {
    let block = Block::default().title("Response").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = match (&app.output, &app.error) {
        (Some(output), _) => {
            let formatted = serde_json::to_string_pretty(output).unwrap_or_default();
            syntax_highlight(&formatted)
        }
        (_, Some(error)) => Text::styled(error, Style::default().fg(Color::Red)),
        _ => Text::raw(""),
    };

    let paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
    f.render_widget(paragraph, inner);
}

fn render_help(app: &AppState, f: &mut Frame, area: Rect) {
    let help_text = match &app.input.mode {
        InputMode::Normal | InputMode::Password => {
            "Enter - Submit | Ctrl+c - Quit"
        }
        InputMode::Command => {
            "Tab - Autocomplete | ↑↓ - Scroll Commands | Enter - Select Command | h - History | Ctrl+c - Quit"
        }
        InputMode::History => {
            "↑↓ - Browse History | Enter - Use Command | Esc - Back | Ctrl+c - Quit"
        }
        InputMode::CommandBuilder { .. } => {
            "Enter - Next Parameter/Submit | Esc - Cancel | Ctrl+c - Quit"
        }
        InputMode::ViewingResponse => {
            "Enter - Return to Commands | c - Copy to Clipboard | e - Export to File | Ctrl+c - Quit"
        }
    };

    let help = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, area);
}

fn syntax_highlight(json: &str) -> Text<'static> {
    let mut spans = Vec::new();
    let mut in_string = false;
    let mut current = String::new();

    for c in json.chars() {
        match c {
            '"' => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                in_string = !in_string;
                spans.push(Span::styled("\"", Style::default().fg(Color::Green)));
            }
            '{' | '}' | '[' | ']' if !in_string => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                spans.push(Span::styled(
                    c.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            ':' if !in_string => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                spans.push(Span::styled(":", Style::default().fg(Color::Cyan)));
            }
            ',' if !in_string => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                spans.push(Span::raw(","));
                spans.push(Span::raw("\n"));
            }
            '\n' if !in_string => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                spans.push(Span::raw("\n"));
            }
            _ => {
                if in_string {
                    spans.push(Span::styled(
                        c.to_string(),
                        Style::default().fg(Color::Green),
                    ));
                } else {
                    current.push(c);
                }
            }
        }
    }

    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    Text::from(Line::from(spans))
}
