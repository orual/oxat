# oxat

A terminal-based API client for Bluesky's AT Protocol (atproto) built in Rust, featuring an interactive TUI for exploring and testing API endpoints.

## Features

- Interactive command selection and parameter input
- Command history with success/failure tracking
- Automatic command completion
- JSON response formatting with syntax highlighting
- Copy responses to clipboard
- Export responses to files

### Controls

- Navigate available commands with arrow keys
- `Tab` to autocomplete commands
- `h` to view command history
- `Enter` to select/execute commands
- In response view:
  - `c` to copy response to clipboard
  - `e` to export response to file
  - `Enter` to return to command list

## Contributing

Pull requests are welcome. For major changes, please open an issue first to discuss what you would like to change.

## License

This project is licensed under:

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](LICENSE)
