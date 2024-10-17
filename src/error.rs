
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("System call error: {0}")]
    Nix(#[from] nix::Error),
    #[error("Invalid property: {0}")]
    InvalidProperty(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
}
