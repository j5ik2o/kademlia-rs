pub mod error;
pub mod network;
pub mod node;
pub mod node_id;
pub mod protocol;
pub mod routing;
pub mod storage;

pub use error::Error;
pub use storage::{MemoryStorage, Storage};
pub type Result<T> = std::result::Result<T, Error>;

/// Initialize tracing subscriber for logging
///
/// This function should be called at the start of any binary using this library.
/// It sets up a tracing subscriber with a filter based on the RUST_LOG environment variable.
pub fn init_tracing() {
  use tracing_subscriber::{fmt, EnvFilter};

  fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_target(true)
    .init();

  tracing::info!("Kademlia DHT logging initialized");
}
