use std::net::SocketAddr;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use tokio::time::sleep;

use kademlia::network::Node;

// Use a custom main function to have more control over the runtime
fn main() -> kademlia::Result<()> {
  // Create a tokio runtime explicitly
  let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

  // Run the async main function
  runtime.block_on(async_main())
}

// Kademlia CLI arguments
#[derive(Parser)]
#[command(name = "Kademlia Node")]
#[command(author = "Kademlia Implementation")]
#[command(version = "0.1.0")]
#[command(about = "A simple Kademlia DHT implementation", long_about = None)]
struct Cli {
  /// Port to listen on
  #[arg(short, long, default_value = "8000")]
  port: u16,

  /// Subcommand to execute
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Run as a bootstrap node
  Bootstrap,

  /// Join an existing Kademlia network
  Join(JoinArgs),

  /// Store a key-value pair
  Store(StoreArgs),

  /// Get a value by key
  Get(GetArgs),
}

#[derive(Args)]
struct JoinArgs {
  /// Bootstrap node address
  #[arg(short, long)]
  bootstrap: String,
}

#[derive(Args)]
struct StoreArgs {
  /// Bootstrap node address
  #[arg(short, long)]
  bootstrap: String,

  /// Key to store
  #[arg(short, long)]
  key: String,

  /// Value to store
  #[arg(short, long)]
  value: String,
}

#[derive(Args)]
struct GetArgs {
  /// Bootstrap node address
  #[arg(short, long)]
  bootstrap: String,

  /// Key to lookup
  #[arg(short, long)]
  key: String,
}

async fn async_main() -> kademlia::Result<()> {
  // Initialize structured logging with tracing
  kademlia::init_tracing();

  // Parse command line arguments
  let cli = Cli::parse();

  // Set up local node address
  let addr = format!("127.0.0.1:{}", cli.port).parse::<SocketAddr>().unwrap();

  // Handle the subcommand
  match cli.command {
    Command::Bootstrap => {
      // Run as a bootstrap node
      tracing::info!(node_addr = %addr, "Starting bootstrap node");
      let node = Node::with_udp(addr).await?;
      node.start().await?;

      // Keep the node running
      tracing::info!("Bootstrap node running. Press Ctrl+C to exit.");
      loop {
        sleep(Duration::from_secs(1)).await;
      }
    }
    Command::Join(args) => {
      // Join an existing network
      let bootstrap_addr = args.bootstrap.parse::<SocketAddr>().unwrap();
      tracing::info!(bootstrap_addr = %bootstrap_addr, "Joining network via bootstrap node");

      let node = Node::with_udp(addr).await?;
      node.start().await?;

      tracing::info!("Connecting to bootstrap node");
      node.join(bootstrap_addr).await?;
      tracing::info!("Successfully joined the Kademlia network");

      // Keep the node running
      tracing::info!("Node running. Press Ctrl+C to exit.");
      loop {
        sleep(Duration::from_secs(1)).await;
      }
    }
    Command::Store(args) => {
      // Store a key-value pair
      let bootstrap_addr = args.bootstrap.parse::<SocketAddr>().unwrap();
      let key = args.key;
      let value = args.value;

      tracing::info!(key = %key, value = %value, "Storing key-value pair");

      let node = Node::with_udp(addr).await?;
      node.start().await?;

      tracing::info!("Connecting to bootstrap node");
      node.join(bootstrap_addr).await?;
      tracing::info!("Successfully joined the Kademlia network");

      tracing::info!("Storing key-value pair");
      tracing::info!("Running store operation - this might take a few seconds");
      match tokio::time::timeout(
        Duration::from_secs(10),
        node.store(key.as_bytes(), value.as_bytes().to_vec()),
      )
      .await
      {
        Ok(result) => match result {
          Ok(_) => tracing::info!(key = %key, value = %value, "Successfully stored key-value pair"),
          Err(e) => tracing::error!(key = %key, error = ?e, "Error while storing key"),
        },
        Err(_) => {
          tracing::warn!("Storage operation timed out after 10 seconds. Continuing anyway")
        }
      };

      // IMPORTANT: Wait for a short period to ensure all responses are received
      tracing::info!("Waiting for 1 second to ensure all responses are received");
      tokio::time::sleep(Duration::from_secs(1)).await;

      // Exit the program normally, allowing the runtime to shut down cleanly
      tracing::info!("Operation completed successfully. Exiting");
      return Ok(());
    }
    Command::Get(args) => {
      // Get a value by key
      let bootstrap_addr = args.bootstrap.parse::<SocketAddr>().unwrap();
      let key = args.key;

      tracing::info!(key = %key, "Looking up key");

      let node = Node::with_udp(addr).await?;
      node.start().await?;

      tracing::info!("Connecting to bootstrap node");
      node.join(bootstrap_addr).await?;
      tracing::info!("Successfully joined the Kademlia network");

      tracing::info!("Looking up key");
      tracing::info!("Running lookup operation - this might take a few seconds");
      match tokio::time::timeout(Duration::from_secs(10), node.get(key.as_bytes())).await {
        Ok(result) => match result {
          Ok(value) => {
            let value_str = String::from_utf8_lossy(&value);
            tracing::info!(key = %key, value = %value_str, "Found value for key");
          }
          Err(e) => {
            tracing::warn!(key = %key, error = %e, "Value not found for key");
          }
        },
        Err(_) => {
          tracing::warn!("Lookup operation timed out after 10 seconds. Continuing anyway");
        }
      }

      // IMPORTANT: Wait for a short period to ensure all responses are received
      tracing::info!("Waiting for 1 second to ensure all responses are received");
      tokio::time::sleep(Duration::from_secs(1)).await;

      // Exit the program normally, allowing the runtime to shut down cleanly
      tracing::info!("Operation completed successfully. Exiting");
      return Ok(());
    }
  }
}
