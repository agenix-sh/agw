// Allow module inception for error types - this is a common Rust pattern
#![allow(clippy::module_name_repetitions)]

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgwError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("RESP protocol error: {0}")]
    RespProtocol(String),

    #[error("Worker error: {0}")]
    #[allow(dead_code)]
    Worker(String),

    #[error("Executor error: {0}")]
    Executor(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),
}

pub type AgwResult<T> = Result<T, AgwError>;
