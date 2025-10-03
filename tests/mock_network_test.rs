use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use kademlia::network::mock::InMemoryNetwork;
use kademlia::network::Node;
use kademlia::node_id::NodeId;
use kademlia::storage::MemoryStorage;
use kademlia::Result;

/// インメモリトランスポートでノード同士のSTORE/GETが成立することを検証する。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_in_memory_transport_end_to_end() -> Result<()> {
  let addr_bootstrap = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 49000);
  let addr_peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 49001);

  let network_bootstrap = InMemoryNetwork::new(addr_bootstrap).await?;
  let network_peer = InMemoryNetwork::new(addr_peer).await?;

  let storage_bootstrap = MemoryStorage::with_name("mock_bootstrap");
  let storage_peer = MemoryStorage::with_name("mock_peer");

  let bootstrap_node = Node::new(
    NodeId::from_socket_addr(&addr_bootstrap),
    addr_bootstrap,
    storage_bootstrap,
    network_bootstrap,
  );
  let peer_node = Node::new(
    NodeId::from_socket_addr(&addr_peer),
    addr_peer,
    storage_peer,
    network_peer,
  );

  bootstrap_node.set_default_request_handler().await?;
  peer_node.set_default_request_handler().await?;

  bootstrap_node.start().await?;
  peer_node.start().await?;

  peer_node.join(addr_bootstrap).await?;

  bootstrap_node.store(b"mock_key", b"mock_value".to_vec()).await?;

  tokio::time::sleep(Duration::from_millis(100)).await;

  let value = peer_node.get(b"mock_key").await?;
  assert_eq!(value, b"mock_value".to_vec());

  Ok(())
}
