//! 오류 — 줄 번호를 들고 다닌다. 원본이 텍스트라 "몇째 줄"이 가장 쓸모 있는 정보다.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: Option<usize>,
    pub msg: String,
}

impl Error {
    pub fn at(line: usize, msg: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            msg: msg.into(),
        }
    }

    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            line: None,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(n) => write!(f, "{n}줄: {}", self.msg),
            None => write!(f, "{}", self.msg),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
