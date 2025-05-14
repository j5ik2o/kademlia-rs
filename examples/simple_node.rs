use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{App, Arg, SubCommand};
use tokio::signal;
use tokio::time::sleep;

use kademlia::network::Node;

// Use a custom main function to have more control over the runtime
fn main() -> kademlia::Result<()> {
  // Create a tokio runtime explicitly
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .unwrap();

  // Run the async main function
  runtime.block_on(async_main())
}

async fn async_main() -> kademlia::Result<()> {
  // Initialize structured logging with tracing
  kademlia::init_tracing();

  let matches = App::new("Kademlia Node")
    .version("0.1.0")
    .author("Kademlia Implementation")
    .about("A simple Kademlia DHT implementation")
    .arg(
      Arg::with_name("port")
        .short("p")
        .long("port")
        .value_name("PORT")
        .help("Port to listen on")
        .takes_value(true)
        .default_value("8000"),
    )
    .subcommand(SubCommand::with_name("bootstrap").about("Run as a bootstrap node"))
    .subcommand(
      SubCommand::with_name("join")
        .about("Join an existing Kademlia network")
        .arg(
          Arg::with_name("bootstrap")
            .short("b")
            .long("bootstrap")
            .value_name("ADDR")
            .help("Bootstrap node address")
            .takes_value(true)
            .required(true),
        ),
    )
    .subcommand(
      SubCommand::with_name("store")
        .about("Store a key-value pair")
        .arg(
          Arg::with_name("bootstrap")
            .short("b")
            .long("bootstrap")
            .value_name("ADDR")
            .help("Bootstrap node address")
            .takes_value(true)
            .required(true),
        )
        .arg(
          Arg::with_name("key")
            .short("k")
            .long("key")
            .value_name("KEY")
            .help("Key to store")
            .takes_value(true)
            .required(true),
        )
        .arg(
          Arg::with_name("value")
            .short("v")
            .long("value")
            .value_name("VALUE")
            .help("Value to store")
            .takes_value(true)
            .required(true),
        ),
    )
    .subcommand(
      SubCommand::with_name("get")
        .about("Get a value by key")
        .arg(
          Arg::with_name("bootstrap")
            .short("b")
            .long("bootstrap")
            .value_name("ADDR")
            .help("Bootstrap node address")
            .takes_value(true)
            .required(true),
        )
        .arg(
          Arg::with_name("key")
            .short("k")
            .long("key")
            .value_name("KEY")
            .help("Key to lookup")
            .takes_value(true)
            .required(true),
        ),
    )
    .get_matches();

  let port = matches.value_of("port").unwrap().parse::<u16>().unwrap();
  let addr = format!("127.0.0.1:{}", port).parse::<SocketAddr>().unwrap();

  if let Some(_) = matches.subcommand_matches("bootstrap") {
    // Run as a bootstrap node
    tracing::info!(node_addr = %addr, "Starting bootstrap node");
    let node = Node::with_udp(addr).await?;
    node.start().await?;

    // Keep the node running
    tracing::info!("Bootstrap node running. Press Ctrl+C to exit.");
    loop {
      sleep(Duration::from_secs(1)).await;
    }
  } else if let Some(matches) = matches.subcommand_matches("join") {
    // Join an existing network
    let bootstrap_addr = matches.value_of("bootstrap").unwrap().parse::<SocketAddr>().unwrap();
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
  } else if let Some(matches) = matches.subcommand_matches("store") {
    // Store a key-value pair
    let bootstrap_addr = matches.value_of("bootstrap").unwrap().parse::<SocketAddr>().unwrap();
    let key = matches.value_of("key").unwrap();
    let value = matches.value_of("value").unwrap();

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

    // 重要: 応答を受信するために少し待機する
    tracing::info!("Waiting for 1 second to ensure all responses are received");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Exit the program normally, allowing the runtime to shut down cleanly
    tracing::info!("Operation completed successfully. Exiting");
    return Ok(());
  } else if let Some(matches) = matches.subcommand_matches("get") {
    // Get a value by key
    let bootstrap_addr = matches.value_of("bootstrap").unwrap().parse::<SocketAddr>().unwrap();
    let key = matches.value_of("key").unwrap();

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
      },
    }

    // 重要: 応答を受信するために少し待機する
    tracing::info!("Waiting for 1 second to ensure all responses are received");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Exit the program normally, allowing the runtime to shut down cleanly
    tracing::info!("Operation completed successfully. Exiting");
    return Ok(());
  } else {
    tracing::warn!("Please specify a subcommand. Use --help for more information.");
    Ok(())
  }
}
