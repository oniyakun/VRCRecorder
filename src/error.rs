use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Glutin(glutin::error::Error),
    Spout(String),
    Encode(String),
    Io(std::io::Error),
    Other(anyhow::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glutin(e) => write!(f, "OpenGL error: {e}"),
            Self::Spout(s) => write!(f, "Spout error: {s}"),
            Self::Encode(s) => write!(f, "Encoder error: {s}"),
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Glutin(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Other(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<glutin::error::Error> for AppError {
    fn from(e: glutin::error::Error) -> Self {
        Self::Glutin(e)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        Self::Spout(s)
    }
}
