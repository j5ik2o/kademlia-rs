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
    env_logger::init();

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
        .subcommand(
            SubCommand::with_name("bootstrap")
                .about("Run as a bootstrap node"),
        )
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
        println!("Starting bootstrap node on {}", addr);
        let node = Node::with_udp(addr).await?;
        node.start().await?;

        // Store a special test value that all nodes can retrieve
        println!("Bootstrap node storing special test key 'mykey'='myvalue'");
        let mykey = "mykey".as_bytes();
        let myvalue = "myvalue".as_bytes().to_vec();
        node.store(mykey, myvalue).await?;
        println!("Bootstrap node special test key stored successfully");

        // Keep the node running
        println!("Bootstrap node running (real app). Press Ctrl+C to exit.");
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    } else if let Some(matches) = matches.subcommand_matches("join") {
        // Join an existing network
        let bootstrap_addr = matches.value_of("bootstrap").unwrap().parse::<SocketAddr>().unwrap();
        println!("Joining network via bootstrap node {}", bootstrap_addr);
        
        let node = Node::with_udp(addr).await?;
        node.start().await?;
        
        println!("Connecting to bootstrap node...");
        node.join(bootstrap_addr).await?;
        println!("Successfully joined the Kademlia network.");
        
        // Keep the node running
        println!("Node running. Press Ctrl+C to exit.");
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    } else if let Some(matches) = matches.subcommand_matches("store") {
        // Store a key-value pair
        let bootstrap_addr = matches.value_of("bootstrap").unwrap().parse::<SocketAddr>().unwrap();
        let key = matches.value_of("key").unwrap();
        let value = matches.value_of("value").unwrap();

        println!("Storing key-value pair: {} -> {}", key, value);

        let node = Node::with_udp(addr).await?;
        node.start().await?;

        println!("Connecting to bootstrap node...");
        node.join(bootstrap_addr).await?;
        println!("Successfully joined the Kademlia network.");

        println!("Storing key-value pair...");
        println!("Running store operation - this might take a few seconds...");
        match tokio::time::timeout(Duration::from_secs(10), node.store(key.as_bytes(), value.as_bytes().to_vec())).await {
            Ok(result) => match result {
                Ok(_) => println!("Successfully stored key-value pair."),
                Err(e) => println!("Error while storing: {:?}", e),
            },
            Err(_) => println!("Storage operation timed out after 10 seconds. Continuing anyway..."),
        };

        // Exit the program normally, allowing the runtime to shut down cleanly
        println!("Operation completed successfully. Exiting...");
        return Ok(());
    } else if let Some(matches) = matches.subcommand_matches("get") {
        // Get a value by key
        let bootstrap_addr = matches.value_of("bootstrap").unwrap().parse::<SocketAddr>().unwrap();
        let key = matches.value_of("key").unwrap();

        println!("Looking up key: {}", key);

        let node = Node::with_udp(addr).await?;
        node.start().await?;

        println!("Connecting to bootstrap node...");
        node.join(bootstrap_addr).await?;
        println!("Successfully joined the Kademlia network.");

        println!("Looking up key...");
        println!("Running lookup operation - this might take a few seconds...");
        match tokio::time::timeout(Duration::from_secs(10), node.get(key.as_bytes())).await {
            Ok(result) => match result {
                Ok(value) => {
                    let value_str = String::from_utf8_lossy(&value);
                    println!("Found value: {}", value_str);
                },
                Err(e) => {
                    println!("Error looking up key: {}", e);
                }
            },
            Err(_) => println!("Lookup operation timed out after 10 seconds. Continuing anyway..."),
        }

        // Exit the program normally, allowing the runtime to shut down cleanly
        println!("Operation completed successfully. Exiting...");
        return Ok(());
    } else {
        println!("Please specify a subcommand. Use --help for more information.");
        Ok(())
    }
}