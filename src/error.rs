use miette::Diagnostic;
use std::{error::Error, fmt::Display};

#[derive(Debug, Diagnostic)]
pub enum AppError {
    #[diagnostic(code(bsky::auth))]
    Auth {
        #[source_code]
        src: String,
        #[label("auth failed")]
        err_span: (usize, usize),
        msg: String,
    },

    #[diagnostic(code(bsky::request))]
    Request {
        #[source_code]
        src: String,
        #[label("request failed")]
        err_span: (usize, usize),
        msg: String,
    },

    #[diagnostic(code(bsky::terminal))]
    Terminal {
        #[source_code]
        src: String,
        #[label("terminal error")]
        err_span: (usize, usize),
        msg: String,
    },
}

pub type AppResult<T> = miette::Result<T>;

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Auth { msg, .. } => write!(f, "Auth error: {}", msg),
            AppError::Request { msg, .. } => write!(f, "Request error: {}", msg),
            AppError::Terminal { msg, .. } => write!(f, "Terminal error: {}", msg),
        }
    }
}

impl Error for AppError {}
