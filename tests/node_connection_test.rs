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
async fn test_node_startup_and_connection() {
  // Add a timeout to prevent test from hanging
  match timeout(Duration::from_secs(10), async {
    // Start a bootstrap node
    let bootstrap_port = get_available_port().await;
    let bootstrap_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), bootstrap_port);

    println!("Starting bootstrap node on {}", bootstrap_addr);
    let bootstrap_node = Node::with_udp(bootstrap_addr).await.unwrap();
    bootstrap_node.start().await.unwrap();

    // Give the bootstrap node some time to start
    sleep(Duration::from_millis(500)).await;

    // Start a second node and connect to the bootstrap node
    let second_port = get_available_port().await;
    let second_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), second_port);

    println!("Starting second node on {}", second_addr);
    let second_node = Node::with_udp(second_addr).await.unwrap();
    second_node.start().await.unwrap();

    // Connect to the bootstrap node
    println!("Connecting second node to bootstrap node");
    let result = second_node.join(bootstrap_addr).await;
    assert!(result.is_ok(), "Failed to join the network: {:?}", result.err());

    // Give some time for the connection to establish
    sleep(Duration::from_millis(500)).await;

    println!("Connection test completed successfully");
  })
  .await
  {
    Ok(_) => println!("Test completed within timeout"),
    Err(_) => panic!("Test timed out after 10 seconds"),
  }
}
