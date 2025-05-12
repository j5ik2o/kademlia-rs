pub mod node_id;
pub mod node;
pub mod routing;
pub mod protocol;
pub mod storage;
pub mod error;
pub mod network;

pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;