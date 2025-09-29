use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, RwLock};
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
  protocol: Arc<Protocol<S, N>>,
}

// Implement Clone for Node
impl<S: Storage, N: Network> Clone for Node<S, N> {
  fn clone(&self) -> Self {
    Node {
      node_id: self.node_id.clone(),
      addr: self.addr,
      routing_table: Arc::clone(&self.routing_table),
      storage: Arc::clone(&self.storage),
      network: Arc::clone(&self.network),
      protocol: Arc::clone(&self.protocol),
    }
  }
}

impl<S: Storage, N: Network> Node<S, N> {
  /// Create a new Kademlia node
  pub fn new(node_id: NodeId, addr: SocketAddr, storage: S, network: N) -> Self {
    let routing_table = Arc::new(RwLock::new(RoutingTable::new(node_id.clone())));
    let storage = Arc::new(RwLock::new(storage));
    let network = Arc::new(network);

    let protocol = Arc::new(Protocol::new(
      node_id.clone(),
      addr,
      routing_table.clone(),
      storage.clone(),
      network.clone(),
    ));

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
    self.protocol.as_ref()
  }

  /// Start the node's background tasks
  pub async fn start(&self) -> Result<()> {
    // This is a simplified implementation - in a real system we would:
    // 1. Set up request handlers for incoming network messages
    // 2. Start background maintenance tasks
    tracing::info!(node_addr = %self.addr, "Node started");

    Ok(())
  }

  /// Handle an incoming request message
  pub async fn handle_request(&self, req: RequestMessage, from: SocketAddr) -> Result<()> {
    self.protocol.handle_request(req, from).await
  }

  /// Store a key-value pair in the DHT
  pub async fn store(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
    // Use a consistent hashing method for the key
    // The caller passes binary keys to this method,
    // but we always process them using the same hash algorithm

    // Human-readable key format (for debugging)
    let key_str = if key.iter().all(|&b| b >= 32 && b <= 126) {
      String::from_utf8_lossy(key).to_string()
    } else {
      format!("<binary key of length {}>", key.len())
    };

    tracing::info!(key = %key_str, "Storing key");

    // Convert to NodeId (using consistent hashing)
    let key_id = NodeId::from_bytes(key);

    // Debug information
    tracing::debug!(
      original_key = %key_str,
      key_bytes = ?key,
      hashed_key_id = %key_id,
      key_hex = %key_id.to_hex(),
      "Key conversion details"
    );

    // Value content (for debugging)
    let value_str = if value.iter().all(|&b| b >= 32 && b <= 126) {
      format!("\"{}\"", String::from_utf8_lossy(&value))
    } else {
      format!("<binary data of length {}>", value.len())
    };
    tracing::debug!(value_content = %value_str, "Value details");

    // First, store it locally
    tracing::info!("Storing value locally");
    // Set a longer timeout (30 seconds)
    let result = match tokio::time::timeout(
      Duration::from_secs(30),
      self.protocol.store_value(key_id.clone(), value.clone()),
    )
    .await
    {
      Ok(r) => {
        tracing::info!("Local store operation completed");
        r
      }
      Err(_) => {
        tracing::warn!("Local store operation timed out, but continuing");
        Ok(())
      }
    };

    // Also ensure it's stored on bootstrap nodes by directly sending to known nodes
    tracing::debug!("Getting nodes from routing table");
    let nodes = {
      let table = self.routing_table.read().await;
      table.get_all_nodes()
    };

    tracing::info!(
      node_count = nodes.len(),
      "Additionally sending STORE to nodes in routing table"
    );
    for node in &nodes {
      if node.id != self.node_id {
        tracing::debug!(target_node = %node.id, "Directly sending STORE to node");
        // Set a longer timeout (30 seconds)
        match tokio::time::timeout(
          Duration::from_secs(30),
          self.protocol.store(node, key_id.clone(), value.clone()),
        )
        .await
        {
          Ok(Ok(_)) => tracing::debug!(node_id = %node.id, "Successfully stored on node"),
          Ok(Err(e)) => tracing::warn!(node_id = %node.id, error = ?e, "Failed to store on node"),
          Err(_) => tracing::warn!(node_id = %node.id, "Timeout storing on node"),
        }
      }
    }

    tracing::info!("Store operation complete");
    result
  }

  /// Retrieve a value from the DHT by key
  pub async fn get(&self, key: &[u8]) -> Result<Vec<u8>> {
    // Human-readable key format (for debugging)
    let key_str = if key.iter().all(|&b| b >= 32 && b <= 126) {
      String::from_utf8_lossy(key).to_string()
    } else {
      format!("<binary key of length {}>", key.len())
    };

    tracing::info!(key = %key_str, "Looking up key");

    // Convert to NodeId (using consistent hashing)
    let key_id = NodeId::from_bytes(key);

    // Debug information
    tracing::debug!(
      original_key = %key_str,
      key_bytes = ?key,
      hashed_key_id = %key_id,
      key_hex = %key_id.to_hex(),
      "Key conversion details for lookup"
    );

    // プロトコルのfind_valueメソッドを呼び出す（タイムアウトなし）
    tracing::info!("Looking up value through protocol");
    let find_result = self.protocol.find_value(&key_id).await;

    match find_result {
      Ok(Some(value)) => {
        tracing::info!("Found value through protocol find_value");
        return Ok(value);
      }
      _ => {
        tracing::warn!("Value not found via protocol find_value");
      }
    }

    tracing::warn!("Value not found anywhere");
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
      tracing::info!("First added bootstrap node directly to routing table");
    }

    // Ping the bootstrap node to validate it
    match self.protocol.ping(&bootstrap_node).await {
      Ok(response) => {
        tracing::info!(bootstrap_id = %response.sender().id, "Bootstrap node responded");

        // Manually update our routing table with the bootstrap node from response
        {
          let mut table = self.routing_table.write().await;
          let _ = table.update(response.sender().clone());
          let node_count = table.node_count();
          tracing::info!(
            node_count = node_count,
            "Then added bootstrap node from response to routing table"
          );

          // Debug: Print all nodes
          let all_nodes = table.get_all_nodes();
          tracing::debug!(node_count = all_nodes.len(), "Current routing table");
          for (i, node) in all_nodes.iter().enumerate() {
            tracing::debug!(
              index = i + 1,
              node_id = %node.id,
              node_addr = %node.addr,
              "Routing table entry"
            );
          }
        }

        // Find the closest nodes to ourselves to populate our routing table
        let closest = self.protocol.find_node(&self.node_id).await?;
        tracing::info!(node_count = closest.len(), "Found closest nodes to self");

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

/// Convenience implementation for using UDP transport with memory storage
impl Node<MemoryStorage, UdpNetwork> {
  /// Create a new Kademlia node with UDP transport and memory storage
  pub async fn with_udp(addr: SocketAddr) -> Result<Self> {
    let node_id = NodeId::from_socket_addr(&addr);

    // Create the storage
    let storage = MemoryStorage::with_name(&format!("node_{}", addr.port()));

    // Create the network
    let network = UdpNetwork::new(addr).await?;

    let (request_tx, mut request_rx) = mpsc::channel::<(RequestMessage, SocketAddr)>(128);
    network.set_request_handler(request_tx).await;

    // Add debug log to verify storage synchronization for testing
    tracing::debug!("Created node with shared storage");

    // Create the node
    let node = Node::new(node_id, addr, storage, network);

    // Bridge incoming requests from the network to the protocol handler
    let protocol = Arc::clone(&node.protocol);
    tokio::spawn(async move {
      tracing::info!("Starting UDP request dispatch loop");
      while let Some((request, from)) = request_rx.recv().await {
        if let Err(err) = protocol.handle_request(request, from).await {
          tracing::warn!(error = ?err, "Failed to handle incoming request");
        }
      }
      tracing::info!("UDP request dispatch loop stopped");
    });

    Ok(node)
  }
}
