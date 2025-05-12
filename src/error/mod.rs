use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("IO error: {0}")]
  Io(#[from] std::io::Error),

  #[error("Serialization error: {0}")]
  Serialization(#[from] serde_json::Error),

  #[error("Bincode error: {0}")]
  Bincode(#[from] bincode::Error),

  #[error("Node not found")]
  NodeNotFound,

  #[error("Value not found")]
  ValueNotFound,

  #[error("Network error: {0}")]
  Network(String),

  #[error("Timeout error")]
  Timeout,

  #[error("Invalid node ID")]
  InvalidNodeId,

  #[error("Other error: {0}")]
  Other(String),
}
