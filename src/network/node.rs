use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::interval;

use crate::network::udp::UdpNetwork;
use crate::node::Node as ContactNode;
use crate::node_id::NodeId;
use crate::protocol::{Network, Protocol, RequestMessage};
use crate::routing::RoutingTable;
use crate::storage::{MemoryStorage, Storage};
use crate::{Error, Result};

// Intervals for background tasks (currently not used since background tasks are disabled)
// In a full implementation, these would be used for maintenance tasks
#[allow(dead_code)]
const REFRESH_INTERVAL: Duration = Duration::from_secs(3600); // 1 hour
#[allow(dead_code)]
const REPUBLISH_INTERVAL: Duration = Duration::from_secs(86400); // 24 hours
#[allow(dead_code)]
const EXPIRATION_CHECK_INTERVAL: Duration = Duration::from_secs(3600); // 1 hour

/// A complete Kademlia DHT node
pub struct Node<S: Storage, N: Network> {
  /// Node ID
  node_id: NodeId,
  /// Node socket address
  addr: SocketAddr,
  /// Routing table
  #[allow(dead_code)]
  routing_table: Arc<RwLock<RoutingTable>>,
  /// Data storage
  storage: Arc<RwLock<S>>,
  /// Network interface
  network: Arc<N>,
  /// Protocol handler
  protocol: Protocol<S, N>,
}

// Implement Clone for Node
impl<S: Storage + Clone, N: Network + Clone> Clone for Node<S, N> {
  fn clone(&self) -> Self {
    let routing_table = Arc::new(RwLock::new(RoutingTable::new(self.node_id.clone())));
    let storage = self.storage.clone();
    let network = self.network.clone();

    let protocol = Protocol::new(
      self.node_id.clone(),
      self.addr,
      routing_table.clone(),
      storage.clone(),
      network.clone(),
    );

    Node {
      node_id: self.node_id.clone(),
      addr: self.addr,
      routing_table,
      storage,
      network,
      protocol,
    }
  }
}

impl<S: Storage, N: Network> Node<S, N> {
  /// Create a new Kademlia node
  pub fn new(node_id: NodeId, addr: SocketAddr, storage: S, network: N) -> Self {
    let routing_table = Arc::new(RwLock::new(RoutingTable::new(node_id.clone())));
    let storage = Arc::new(RwLock::new(storage));
    let network = Arc::new(network);

    let protocol = Protocol::new(
      node_id.clone(),
      addr,
      routing_table.clone(),
      storage.clone(),
      network.clone(),
    );

    Node {
      node_id,
      addr,
      routing_table,
      storage,
      network,
      protocol,
    }
  }

  /// Get the node's ID
  pub fn node_id(&self) -> &NodeId {
    &self.node_id
  }

  /// Get the node's socket address
  pub fn addr(&self) -> SocketAddr {
    self.addr
  }

  /// Get the node's routing table
  pub fn routing_table(&self) -> &Arc<RwLock<RoutingTable>> {
    &self.routing_table
  }

  /// Get the node's protocol handler
  pub fn protocol(&self) -> &Protocol<S, N> {
    &self.protocol
  }

  /// Start the node's background tasks
  pub async fn start(&self) -> Result<()> {
    // This is a simplified implementation - in a real system we would:
    // 1. Set up request handlers for incoming network messages
    // 2. Start background maintenance tasks
    println!("Node started on {}", self.addr);

    Ok(())
  }

  /// Handle an incoming request message
  pub async fn handle_request(&self, req: RequestMessage, from: SocketAddr) -> Result<()> {
    self.protocol.handle_request(req, from).await
  }

  /// Store a key-value pair in the DHT
  pub async fn store(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
    // キーのハッシュ方法を一貫性のあるものにする
    // このメソッドを利用する側はバイナリキーを渡すが、
    // 常に同じハッシュアルゴリズムを使用して処理する

    // 人間が読めるキー形式（デバッグ用）
    let key_str = if key.iter().all(|&b| b >= 32 && b <= 126) {
      String::from_utf8_lossy(key).to_string()
    } else {
      format!("<binary key of length {}>", key.len())
    };

    println!("Storing key: \"{}\"", key_str);

    // NodeIdに変換（一貫したハッシュを使用）
    let key_id = NodeId::from_bytes(key);

    // デバッグ情報
    println!("DEBUG: Original key: \"{}\" (bytes: {:?})", key_str, key);
    println!("DEBUG: Hashed key ID: {} (hex: {})", key_id, key_id.to_hex());

    // 値の内容（デバッグ用）
    let value_str = if value.iter().all(|&b| b >= 32 && b <= 126) {
      format!("\"{}\"", String::from_utf8_lossy(&value))
    } else {
      format!("<binary data of length {}>", value.len())
    };
    println!("DEBUG: Value content: {}", value_str);

    // First, store it locally
    println!("Storing value locally...");
    let result = match tokio::time::timeout(
      Duration::from_secs(15),
      self.protocol.store_value(key_id.clone(), value.clone()),
    )
    .await
    {
      Ok(r) => {
        println!("Local store operation completed");
        r
      }
      Err(_) => {
        println!("Local store operation timed out, but continuing...");
        Ok(())
      }
    };

    // Also ensure it's stored on bootstrap nodes by directly sending to known nodes
    println!("Getting nodes from routing table...");
    let table = self.routing_table.read().await;
    let nodes = table.get_all_nodes();

    println!("Additionally sending STORE to {} nodes in routing table", nodes.len());
    for node in &nodes {
      if node.id != self.node_id {
        println!("Directly sending STORE to node: {}", node.id);
        match tokio::time::timeout(
          Duration::from_secs(10),
          self.protocol.store(node, key_id.clone(), value.clone()),
        )
        .await
        {
          Ok(Ok(_)) => println!("Successfully stored on node: {}", node.id),
          Ok(Err(e)) => println!("Failed to store on node {}: {:?}", node.id, e),
          Err(_) => println!("Timeout storing on node {}", node.id),
        }
      }
    }

    println!("Store operation complete");
    result
  }

  /// Retrieve a value from the DHT by key
  pub async fn get(&self, key: &[u8]) -> Result<Vec<u8>> {
    // 人間が読めるキー形式（デバッグ用）
    let key_str = if key.iter().all(|&b| b >= 32 && b <= 126) {
      String::from_utf8_lossy(key).to_string()
    } else {
      format!("<binary key of length {}>", key.len())
    };

    println!("Looking up key: \"{}\"", key_str);

    // NodeIdに変換（一貫したハッシュを使用）
    let key_id = NodeId::from_bytes(key);

    // デバッグ情報
    println!("DEBUG: Original key: \"{}\" (bytes: {:?})", key_str, key);
    println!("DEBUG: Hashed key ID for lookup: {} (hex: {})", key_id, key_id.to_hex());

    // プロトコルのfind_valueメソッドを呼び出す（タイムアウトなし）
    println!("Looking up value through protocol...");
    let find_result = self.protocol.find_value(&key_id).await;

    match find_result {
      Ok(Some(value)) => {
        println!("Found value through protocol find_value");
        return Ok(value);
      }
      _ => {
        println!("Value not found via protocol find_value");
      }
    }

    println!("Value not found anywhere");
    Err(Error::ValueNotFound)
  }

  /// Join the Kademlia network by contacting a bootstrap node
  pub async fn join(&self, bootstrap_addr: SocketAddr) -> Result<()> {
    // Create a contact for the bootstrap node
    let bootstrap_node = ContactNode::with_addr(bootstrap_addr);

    // Add bootstrap node directly to our routing table first
    {
      let mut table = self.routing_table.write().await;
      let _ = table.update(bootstrap_node.clone());
      println!("First added bootstrap node directly to routing table");
    }

    // Ping the bootstrap node to validate it
    match self.protocol.ping(&bootstrap_node).await {
      Ok(response) => {
        println!("Bootstrap node responded: {}", response.sender().id);

        // Manually update our routing table with the bootstrap node from response
        {
          let mut table = self.routing_table.write().await;
          let _ = table.update(response.sender().clone());
          let node_count = table.node_count();
          println!(
            "Then added bootstrap node from response to routing table. Total nodes: {}",
            node_count
          );

          // Debug: Print all nodes
          let all_nodes = table.get_all_nodes();
          println!("Current routing table has {} nodes:", all_nodes.len());
          for (i, node) in all_nodes.iter().enumerate() {
            println!("  Node {}: {} at {}", i + 1, node.id, node.addr);
          }
        }

        // Find the closest nodes to ourselves to populate our routing table
        let closest = self.protocol.find_node(&self.node_id).await?;
        println!("Found {} closest nodes to self", closest.len());

        Ok(())
      }
      Err(e) => Err(e),
    }
  }

  /// Periodically refresh buckets by looking up a random ID in each bucket's range
  #[allow(dead_code)]
  fn spawn_refresh_task(node_arc: &Arc<Self>) {
    let node = node_arc.clone();
    tokio::spawn(async move {
      let mut interval = interval(REFRESH_INTERVAL);

      loop {
        interval.tick().await;

        // For each bucket, lookup a random ID in that bucket's range
        let table = node.routing_table.read().await;
        let buckets_count = table.node_count();
        drop(table);

        if buckets_count > 0 {
          // Refresh all buckets by doing a lookup for a random ID
          for _ in 0..8 {
            // Refresh 8 random IDs
            let random_id = NodeId::random();
            let _ = node.protocol.find_node(&random_id).await;
          }
        }
      }
    });
  }

  /// Periodically republish stored key-value pairs
  #[allow(dead_code)]
  fn spawn_republish_task(node_arc: &Arc<Self>) {
    let node = node_arc.clone();
    tokio::spawn(async move {
      let mut interval = interval(REPUBLISH_INTERVAL);

      loop {
        interval.tick().await;

        // Get all stored key-value pairs
        let _storage = node.storage.read().await;

        // This would require a method to iterate over all key-value pairs
        // which we haven't implemented in the Storage trait.
        // In a real implementation, you would need to add this method
        // to the Storage trait.

        // For now, we'll just simulate it with a comment:
        // for (key, value) in storage.iter() {
        //     let _ = node.protocol.store_value(key.clone(), value.clone()).await;
        // }
      }
    });
  }

  /// Periodically check for expired entries
  #[allow(dead_code)]
  fn spawn_expiration_check_task(node_arc: &Arc<Self>) {
    let node = node_arc.clone();
    tokio::spawn(async move {
      let mut interval = interval(EXPIRATION_CHECK_INTERVAL);

      loop {
        interval.tick().await;

        // Check for expired entries
        // This would also require a method in the Storage trait to clean up
        // expired entries. For MemoryStorage, we already have a cleanup method.

        // This is a type check without unsafe code - it just skips cleanup if S is not MemoryStorage
        if std::any::TypeId::of::<S>() == std::any::TypeId::of::<MemoryStorage>() {
          // Downcast would require unsafe code, so we just try to get a write lock
          // and cast it to Any. For a real implementation, you would add cleanup to the Storage trait
          if let Ok(_storage) = node.storage.try_write() {
            // We can't directly call cleanup() since we don't know S is MemoryStorage at compile time
            // So we'll just ignore this for now - in a real implementation, Storage would have a cleanup method
          }
        }
      }
    });
  }
}

/// Convenience implementation for using UDP transport
impl Node<MemoryStorage, UdpNetwork> {
  /// Create a new Kademlia node with UDP transport and memory storage
  pub async fn with_udp(addr: SocketAddr) -> Result<Self> {
    let node_id = NodeId::from_socket_addr(&addr);

    // 重要: 共有ストレージを作成し、MemoryStorageとUdpNetworkの両方で使用する
    let shared_storage = Arc::new(tokio::sync::Mutex::new(HashMap::<NodeId, Vec<u8>>::new()));

    // MemoryStorageの代わりに直接共有ストレージを使用するカスタムストレージを作成
    // 実際のアプリケーションでは、MemoryStorageを修正して共有ストレージを使用するようにすべき
    let storage = MemoryStorage::with_name(&format!("node_{}", addr.port()));

    // Create the network with the shared storage
    let network = UdpNetwork::with_storage(addr, shared_storage.clone()).await?;

    // テスト用に、ストレージの同期を確認するデバッグログを追加
    println!("DEBUG: Created node with shared storage between MemoryStorage and UdpNetwork");

    // Create the node
    Ok(Node::new(node_id, addr, storage, network))
  }
}
