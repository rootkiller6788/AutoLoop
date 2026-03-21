use std::{error::Error, fmt};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ContractError {
    Policy(PolicyError),
    Runtime(RuntimeError),
    Verification(VerificationError),
    InvalidTransition(String),
    Storage(String),
    Internal(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerificationError {
    pub code: String,
    pub message: String,
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(err) => write!(f, "policy error [{}]: {}", err.code, err.message),
            Self::Runtime(err) => write!(f, "runtime error [{}]: {}", err.code, err.message),
            Self::Verification(err) => {
                write!(f, "verification error [{}]: {}", err.code, err.message)
            }
            Self::InvalidTransition(msg) => write!(f, "invalid transition: {msg}"),
            Self::Storage(msg) => write!(f, "storage error: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl Error for ContractError {}
