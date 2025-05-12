use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::time::{sleep, timeout};

use kademlia::network::Node;

// Helper function to get a random available port
async fn get_available_port() -> u16 {
    // Use port 0 to let the OS assign an available port
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.local_addr().unwrap().port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_store_and_retrieve() -> kademlia::Result<()> {
    // Add a timeout to prevent test from hanging
    timeout(Duration::from_secs(5), async {
    // Start a bootstrap node
    let bootstrap_port = get_available_port().await;
    let bootstrap_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), bootstrap_port);

    println!("Starting bootstrap node on {}", bootstrap_addr);
    let bootstrap_node = Node::with_udp(bootstrap_addr).await?;
    bootstrap_node.start().await?;

    // Test only local storage for simplicity
    let test_key = "test_key";
    let test_value = "test_value";

    println!("Storing key-value pair locally: {} -> {}", test_key, test_value);
    bootstrap_node.store(test_key.as_bytes(), test_value.as_bytes().to_vec()).await?;

    // Wait for the store to complete
    sleep(Duration::from_millis(100)).await;

    // Retrieve the value from the same node
    println!("Retrieving value from the same node");
    let retrieve_result = bootstrap_node.get(test_key.as_bytes()).await;

    assert!(retrieve_result.is_ok(), "Failed to retrieve value: {:?}", retrieve_result.err());

    let retrieved_value = retrieve_result?;
    let retrieved_str = String::from_utf8_lossy(&retrieved_value).to_string();

    assert_eq!(retrieved_str, test_value, "Retrieved value does not match stored value");

    println!("Store and retrieve test completed successfully");
    Ok(())
    }).await.expect("Test timed out after 5 seconds")
}