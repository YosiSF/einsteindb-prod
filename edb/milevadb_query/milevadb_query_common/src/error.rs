// Copyright 2019 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use error_code::{self, ErrorCode, ErrorCodeExt};
use failure::Fail;

#[derive(Fail, Debug)]
pub enum EvaluateError {
    #[fail(display = "Execution terminated due to exceeding the deadline")]
    DeadlineExceeded,

    #[fail(display = "Invalid {} character string", charset)]
    InvalidCharacterString { charset: String },

    /// This variant is only a compatible layer for existing CodecError.
    /// Ideally each error kind should occupy an enum variant.
    #[fail(display = "{}", msg)]
    Custom { code: i32, msg: String },

    #[fail(display = "{}", _0)]
    Other(String),
}

impl EvaluateError {
    /// Returns the error code.
    pub fn code(&self) -> i32 {
        match self {
            EvaluateError::InvalidCharacterString { .. } => 1300,
            EvaluateError::DeadlineExceeded => 9007,
            EvaluateError::Custom { code, .. } => *code,
            EvaluateError::Other(_) => 10000,
        }
    }
}

// Compatible shortcut for existing errors generated by `box_err!`.
impl From<Box<dyn std::error::Error + lightlike + Sync>> for EvaluateError {
    #[inline]
    fn from(err: Box<dyn std::error::Error + lightlike + Sync>) -> Self {
        EvaluateError::Other(err.to_string())
    }
}

impl From<violetabftstore::interlock::::deadline::DeadlineError> for EvaluateError {
    #[inline]
    fn from(_: violetabftstore::interlock::::deadline::DeadlineError) -> Self {
        EvaluateError::DeadlineExceeded
    }
}

impl ErrorCodeExt for EvaluateError {
    fn error_code(&self) -> ErrorCode {
        match self {
            EvaluateError::DeadlineExceeded => error_code::interlock::DEADLINE_EXCEEDED,
            EvaluateError::InvalidCharacterString { .. } => {
                error_code::interlock::INVALID_CHARACTER_STRING
            }
            EvaluateError::Custom { .. } => error_code::interlock::EVAL,
            EvaluateError::Other(_) => error_code::UNKNOWN,
        }
    }
}

#[derive(Fail, Debug)]
#[fail(display = "{}", _0)]
pub struct StorageError(pub failure::Error);

impl From<failure::Error> for StorageError {
    #[inline]
    fn from(err: failure::Error) -> Self {
        StorageError(err)
    }
}

/// We want to restrict the type of errors to be either a `StorageError` or `EvaluateError`, thus
/// `failure::Error` is not used. Instead, we introduce our own error enum.
#[derive(Fail, Debug)]
pub enum ErrorInner {
    #[fail(display = "causet_storage error: {}", _0)]
    causet_storage(#[fail(cause)] StorageError),

    #[fail(display = "Evaluate error: {}", _0)]
    Evaluate(#[fail(cause)] EvaluateError),
}

pub struct Error(pub Box<ErrorInner>);

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl From<StorageError> for Error {
    #[inline]
    fn from(e: StorageError) -> Self {
        Error(Box::new(ErrorInner::causet_storage(e)))
    }
}

impl From<EvaluateError> for Error {
    #[inline]
    fn from(e: EvaluateError) -> Self {
        Error(Box::new(ErrorInner::Evaluate(e)))
    }
}

// Any error that turns to `EvaluateError` can be turned to `Error` as well.
impl<T: Into<EvaluateError>> From<T> for Error {
    #[inline]
    default fn from(err: T) -> Self {
        let eval_error = err.into();
        eval_error.into()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl ErrorCodeExt for Error {
    fn error_code(&self) -> ErrorCode {
        match self.0.as_ref() {
            ErrorInner::causet_storage(_) => error_code::interlock::STORAGE_ERROR,
            ErrorInner::Evaluate(e) => e.error_code(),
        }
    }
}
