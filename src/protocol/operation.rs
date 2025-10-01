use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

use crate::node::Node;
use crate::node_id::NodeId;
use crate::protocol::message::{KademliaMessage, MessageId, RequestMessage, ResponseMessage};
use crate::routing::{RoutingTable, UpdateStatus, K};
use crate::storage::Storage;
use crate::{Error, Result};

/// Maximum concurrent RPCs during a node lookup
const ALPHA: usize = 5;

/// Default timeout for RPC operations
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Protocol handler for Kademlia operations
pub struct Protocol<S, N>
where
  S: Storage,
  N: Network, {
  /// Local node ID
  node_id: NodeId,
  /// Local node socket address
  addr: SocketAddr,
  /// Routing table
  routing_table: Arc<RwLock<RoutingTable>>,
  /// Data storage
  storage: Arc<RwLock<S>>,
  /// Network layer
  network: Arc<N>,
  /// Map of pending requests by message ID
  pending_requests: Arc<Mutex<HashMap<MessageId, RequestMessage>>>,
}

/// Network interface for sending and receiving messages
#[async_trait]
pub trait Network: Send + Sync + 'static {
  /// Send a message to a specific node address
  async fn send(&self, to: SocketAddr, message: KademliaMessage) -> Result<()>;

  /// Wait for a response to a specific request
  async fn wait_response(&self, request_id: MessageId, timeout: Duration) -> Result<ResponseMessage>;
}

impl<S, N> Protocol<S, N>
where
  S: Storage,
  N: Network,
{
  /// Create a new protocol handler
  pub fn new(
    node_id: NodeId,
    addr: SocketAddr,
    routing_table: Arc<RwLock<RoutingTable>>,
    storage: Arc<RwLock<S>>,
    network: Arc<N>,
  ) -> Self {
    Protocol {
      node_id,
      addr,
      routing_table,
      storage,
      network,
      pending_requests: Arc::new(Mutex::new(HashMap::new())),
    }
  }

  /// Handle an incoming request
  pub async fn handle_request(&self, req: RequestMessage, from: SocketAddr) -> Result<()> {
    let sender = req.sender().clone();
    let is_new_contact = self.record_contact(&sender).await?;

    match req {
      RequestMessage::Ping { id, sender } => {
        self.handle_ping(id, sender, from).await?;
      }
      RequestMessage::Store { id, sender, key, value } => {
        self.handle_store(id, sender, key, value, from).await?;
      }
      RequestMessage::FindNode { id, sender, target } => {
        self.handle_find_node(id, sender, target, from).await?;
      }
      RequestMessage::FindValue { id, sender, key } => {
        self.handle_find_value(id, sender, key, from).await?;
      }
    }

    if is_new_contact {
      if let Err(err) = self.replicate_to_new_node(&sender).await {
        tracing::debug!(target = %sender.id, ?err, "Failed to replicate data to new node");
      }
    }

    Ok(())
  }

  /// Handle a PING request
  async fn handle_ping(&self, id: MessageId, _sender: Node, from: SocketAddr) -> Result<()> {
    let local_node = Node::new(self.node_id.clone(), self.addr);
    let response = ResponseMessage::Pong {
      request_id: id,
      sender: local_node,
    };

    self.network.send(from, KademliaMessage::Response(response)).await
  }

  /// Handle a STORE request
  async fn handle_store(
    &self,
    id: MessageId,
    _sender: Node,
    key: NodeId,
    value: Vec<u8>,
    from: SocketAddr,
  ) -> Result<()> {
    let mut success = false;
    {
      let mut storage = self.storage.write().await;
      if storage.store(&key, value).is_ok() {
        success = true;
      }
    }

    let local_node = Node::new(self.node_id.clone(), self.addr);
    let response = ResponseMessage::StoreResult {
      request_id: id,
      sender: local_node,
      success,
    };

    self.network.send(from, KademliaMessage::Response(response)).await
  }

  /// Handle a FIND_NODE request
  async fn handle_find_node(&self, id: MessageId, _sender: Node, target: NodeId, from: SocketAddr) -> Result<()> {
    let nodes = {
      let table = self.routing_table.read().await;
      table.get_closest(&target, K)
    };

    let local_node = Node::new(self.node_id.clone(), self.addr);
    let response = ResponseMessage::NodesFound {
      request_id: id,
      sender: local_node,
      nodes,
    };

    self.network.send(from, KademliaMessage::Response(response)).await
  }

  /// Handle a FIND_VALUE request
  async fn handle_find_value(&self, id: MessageId, _sender: Node, key: NodeId, from: SocketAddr) -> Result<()> {
    // Check if we have the value locally
    let value_opt = {
      let mut storage = self.storage.write().await;
      storage.get(&key).ok()
    };

    let local_node = Node::new(self.node_id.clone(), self.addr);
    let response = match value_opt {
      // If we have the value, return it
      Some(value) => ResponseMessage::ValueFound {
        request_id: id,
        sender: local_node,
        value: Some(value),
        nodes: vec![],
      },
      // If we don't have the value, return the k closest nodes
      None => {
        let nodes = {
          let table = self.routing_table.read().await;
          table.get_closest(&key, K)
        };

        ResponseMessage::ValueFound {
          request_id: id,
          sender: local_node,
          value: None,
          nodes,
        }
      }
    };

    self.network.send(from, KademliaMessage::Response(response)).await
  }

  /// Send a PING request to a node
  pub async fn ping(&self, node: &Node) -> Result<ResponseMessage> {
    let id = rand::random::<MessageId>();
    let local_node = Node::new(self.node_id.clone(), self.addr);

    let request = RequestMessage::Ping { id, sender: local_node };

    {
      let mut pending = self.pending_requests.lock().await;
      pending.insert(id, request.clone());
    }

    self.network.send(node.addr, KademliaMessage::Request(request)).await?;

    match timeout(RPC_TIMEOUT, self.network.wait_response(id, RPC_TIMEOUT)).await {
      Ok(result) => {
        let response = result?;

        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        let _ = self.record_contact(response.sender()).await?;

        Ok(response)
      }
      Err(_) => {
        // Timeout, clean up and return error
        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        Err(Error::Timeout)
      }
    }
  }

  /// Store a key-value pair on a node
  pub async fn store(&self, node: &Node, key: NodeId, value: Vec<u8>) -> Result<ResponseMessage> {
    let id = rand::random::<MessageId>();
    let local_node = Node::new(self.node_id.clone(), self.addr);

    let request = RequestMessage::Store {
      id,
      sender: local_node,
      key,
      value,
    };

    {
      let mut pending = self.pending_requests.lock().await;
      pending.insert(id, request.clone());
    }

    self.network.send(node.addr, KademliaMessage::Request(request)).await?;

    match timeout(RPC_TIMEOUT, self.network.wait_response(id, RPC_TIMEOUT)).await {
      Ok(result) => {
        let response = result?;

        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        let _ = self.record_contact(response.sender()).await?;

        Ok(response)
      }
      Err(_) => {
        // Timeout, clean up and return error
        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        Err(Error::Timeout)
      }
    }
  }

  /// Find the k closest nodes to a target
  pub async fn find_node(&self, target: &NodeId) -> Result<Vec<Node>> {
    let mut seen = HashSet::new();

    // Include our own ID in the seen set so we don't query ourselves
    seen.insert(self.node_id.clone());

    // Get the k closest nodes from our routing table
    let mut closest_nodes = {
      let table = self.routing_table.read().await;
      table.get_closest(target, K)
    };

    // Track all nodes seen during the lookup
    for node in &closest_nodes {
      seen.insert(node.id.clone());
    }

    // Use the iterative find algorithm
    let mut active = std::cmp::min(ALPHA, closest_nodes.len());
    let mut looking_for_closest = true;

    while looking_for_closest && !closest_nodes.is_empty() {
      let mut requests = Vec::new();

      // Send concurrent requests for up to ALPHA nodes
      for i in 0..active {
        if i >= closest_nodes.len() {
          break;
        }

        let node = &closest_nodes[i];
        requests.push(self.find_node_rpc(node, target.clone()));
      }

      // Wait for all concurrent requests to complete
      let results = futures::future::join_all(requests).await;
      looking_for_closest = false;

      for (idx, result) in results.into_iter().enumerate() {
        if idx >= closest_nodes.len() {
          continue;
        }

        match result {
          Ok(nodes) => {
            // Check if we found any closer nodes
            let old_closest = if !closest_nodes.is_empty() {
              closest_nodes[0].id.distance(target)
            } else {
              NodeId::random() // Placeholder for empty list
            };

            // Add new nodes to our list
            for node in nodes {
              if !seen.contains(&node.id) {
                seen.insert(node.id.clone());
                closest_nodes.push(node);
              }
            }

            // Re-sort by distance to target
            closest_nodes.sort_by(|a, b| {
              let dist_a = a.id.distance(target);
              let dist_b = b.id.distance(target);
              dist_a.cmp(&dist_b)
            });

            // Truncate to k closest
            if closest_nodes.len() > K {
              closest_nodes.truncate(K);
            }

            // If we have a new closest node, we need to keep looking
            if !closest_nodes.is_empty() && closest_nodes[0].id.distance(target) < old_closest {
              looking_for_closest = true;
            }
          }
          Err(_) => {
            // If a node didn't respond, remove it
            if idx < closest_nodes.len() {
              closest_nodes.remove(idx);
            }
          }
        }
      }

      // Update number of active requests based on remaining nodes
      active = std::cmp::min(ALPHA, closest_nodes.len());
    }

    Ok(closest_nodes)
  }

  /// Send a FIND_NODE RPC to a single node
  async fn find_node_rpc(&self, node: &Node, target: NodeId) -> Result<Vec<Node>> {
    let id = rand::random::<MessageId>();
    let local_node = Node::new(self.node_id.clone(), self.addr);

    let request = RequestMessage::FindNode {
      id,
      sender: local_node,
      target,
    };

    {
      let mut pending = self.pending_requests.lock().await;
      pending.insert(id, request.clone());
    }

    self.network.send(node.addr, KademliaMessage::Request(request)).await?;

    match timeout(RPC_TIMEOUT, self.network.wait_response(id, RPC_TIMEOUT)).await {
      Ok(result) => {
        let response = result?;

        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        let _ = self.record_contact(response.sender()).await?;

        match response {
          ResponseMessage::NodesFound { nodes, .. } => Ok(nodes),
          _ => Err(Error::Other("Unexpected response type".to_string())),
        }
      }
      Err(_) => {
        // Timeout, clean up and return error
        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        Err(Error::Timeout)
      }
    }
  }

  /// Find a value by key
  pub async fn find_value(&self, key: &NodeId) -> Result<Option<Vec<u8>>> {
    // Debug: Display information about the key being searched
    tracing::debug!(key = %key, key_hex = %key.to_hex(), "Looking for key in find_value");

    // First check if we have the value locally
    {
      let mut storage = self.storage.write().await;
      if let Ok(value) = storage.get(key) {
        tracing::info!("Value found in local node storage");
        return Ok(Some(value));
      }
    }

    tracing::info!("Value not found locally, initiating network search");

    // Improvement: Implement a more efficient Kademlia search algorithm
    // Search from nodes closest to the key, and continue exploration when finding even closer nodes

    // Set of nodes to use for initial search
    let mut closest_nodes = {
      let table = self.routing_table.read().await;
      // First find the K nodes closest to the key
      let mut nodes = table.get_closest(key, K);

      if nodes.is_empty() {
        // If no closest nodes are found, use all known nodes
        nodes = table.get_all_nodes();
        tracing::info!(
          node_count = nodes.len(),
          "No closest nodes found, using all known nodes"
        );
      } else {
        tracing::info!(node_count = nodes.len(), key = %key, "Found closest nodes to key");
      }

      nodes
    };

    if closest_nodes.is_empty() {
      tracing::warn!("No known nodes to query. Make sure you've joined the network.");
      return Ok(None);
    }

    // Track nodes that have already been visited
    let mut visited = std::collections::HashSet::new();
    visited.insert(self.node_id.clone());

    // Maximum number of iterations to continue the search
    const MAX_ITERATIONS: usize = 10;

    for iteration in 0..MAX_ITERATIONS {
      tracing::debug!(
        iteration = iteration,
        node_count = closest_nodes.len(),
        "Find value iteration"
      );

      // Filter out nodes that have already been visited in this iteration
      let nodes_to_query: Vec<Node> = closest_nodes
        .iter()
        .filter(|node| !visited.contains(&node.id) && node.id != self.node_id)
        .take(ALPHA) // Query a maximum of ALPHA nodes in parallel
        .cloned()
        .collect();

      if nodes_to_query.is_empty() {
        tracing::debug!(iteration = iteration, "No more nodes to query in iteration");
        break;
      }

      // Send queries to multiple nodes in parallel
      let mut tasks = Vec::new();
      for node in &nodes_to_query {
        visited.insert(node.id.clone());
        tracing::debug!(node_id = %node.id, "Querying node");

        // Create a task to issue RPC to each node
        let node_clone = node.clone();
        let key_clone = key.clone();
        // Only copy what's needed instead of self.clone
        let network = self.network.clone();
        let storage = self.storage.clone();
        let routing_table = self.routing_table.clone();
        let pending_requests = self.pending_requests.clone();
        let node_id = self.node_id.clone();
        let addr = self.addr.clone();

        tasks.push(tokio::spawn(async move {
          // Create a new Protocol instance
          let protocol = Protocol {
            node_id,
            addr,
            storage,
            network,
            routing_table,
            pending_requests,
          };
          match protocol.find_value_rpc(&node_clone, key_clone).await {
            Ok(result) => Some((node_clone, result)),
            Err(_) => {
              tracing::warn!(node_id = %node_clone.id, "Error or timeout querying node");
              None
            }
          }
        }));
      }

      // Wait for the results of all queries
      let mut all_closest_nodes = Vec::new();
      let value_found = false;

      for task in tasks {
        if let Ok(Some((node, (maybe_value, more_nodes)))) = task.await {
          if let Some(value) = maybe_value {
            tracing::info!(node_id = %node.id, "Found value on node");

            // Cache the value locally
            {
              let mut storage = self.storage.write().await;
              let _ = storage.store(key, value.clone());
            }

            tracing::info!(node_id = %node.id, "Value retrieved from node");
            return Ok(Some(value));
          } else {
            tracing::debug!(
                node_id = %node.id,
                closest_nodes = more_nodes.len(),
                "Node returned closest nodes"
            );
            // Add newly found nodes
            for n in more_nodes {
              if !visited.contains(&n.id) {
                all_closest_nodes.push(n);
              }
            }
          }
        }
      }

      // If no value is found, add newly discovered nodes and continue
      if !value_found && !all_closest_nodes.is_empty() {
        tracing::debug!(
          new_nodes = all_closest_nodes.len(),
          "Added new nodes for next iteration"
        );
        closest_nodes.extend(all_closest_nodes);

        // Sort by XOR distance and limit to K nodes
        closest_nodes.sort_by(|a, b| {
          let dist_a = key.distance(&a.id);
          let dist_b = key.distance(&b.id);
          dist_a.cmp(&dist_b)
        });

        if closest_nodes.len() > K {
          closest_nodes.truncate(K);
        }
      } else if !value_found {
        // Exit when no new nodes are found
        tracing::debug!("No new nodes found, stopping search");
        break;
      }
    }

    // Not found anywhere
    tracing::warn!("Value not found in the network after exhaustive search");
    Ok(None)
  }

  /// Send a FIND_VALUE RPC to a single node
  pub async fn find_value_rpc(&self, node: &Node, key: NodeId) -> Result<(Option<Vec<u8>>, Vec<Node>)> {
    let id = rand::random::<MessageId>();
    let local_node = Node::new(self.node_id.clone(), self.addr);

    let request = RequestMessage::FindValue {
      id,
      sender: local_node,
      key,
    };

    {
      let mut pending = self.pending_requests.lock().await;
      pending.insert(id, request.clone());
    }

    self.network.send(node.addr, KademliaMessage::Request(request)).await?;

    // タイムアウト時間を延長
    let extended_timeout = Duration::from_secs(10);
    match timeout(extended_timeout, self.network.wait_response(id, RPC_TIMEOUT)).await {
      Ok(result) => {
        let response = result?;
        tracing::debug!(response = ?response, "Received response in find_value_rpc");

        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        let _ = self.record_contact(response.sender()).await?;

        match response {
          ResponseMessage::ValueFound { value, nodes, .. } => {
            tracing::debug!(
              has_value = value.is_some(),
              nodes_count = nodes.len(),
              "find_value_rpc received ValueFound response"
            );
            if let Some(ref v) = value {
              tracing::trace!(
                value_content = %String::from_utf8_lossy(v),
                "Value content details"
              );
            }
            Ok((value, nodes))
          }
          ResponseMessage::NodesFound { nodes, .. } => {
            tracing::debug!(nodes_count = nodes.len(), "find_value_rpc received NodesFound response");
            // 値が見つからなかった場合は、ノードのリストを返す
            Ok((None, nodes))
          }
          _ => {
            tracing::warn!("find_value_rpc received unexpected response type");
            // 予期しないレスポンスタイプの場合でも、空のノードリストを返す
            Ok((None, vec![]))
          }
        }
      }
      Err(e) => {
        // Timeout, clean up and return error
        tracing::warn!(error = ?e, "find_value_rpc timed out");
        {
          let mut pending = self.pending_requests.lock().await;
          pending.remove(&id);
        }

        // タイムアウトの場合でも、空のノードリストを返す
        Ok((None, vec![]))
      }
    }
  }

  /// Store a key-value pair in the network
  pub async fn store_value(&self, key: NodeId, value: Vec<u8>) -> Result<()> {
    // Store the value locally
    {
      let mut storage = self.storage.write().await;
      storage.store(&key, value.clone())?;
    }

    tracing::info!(key = %key, "Stored value locally for key");

    // Fix: Find the K nodes closest to the key (compliant with Kademlia protocol)
    let nodes = {
      let table = self.routing_table.read().await;

      // First, get the K nodes closest to the key (ideal Kademlia implementation)
      let closest_nodes = table.get_closest(&key, K);
      tracing::info!(node_count = closest_nodes.len(), key = %key, "Found closest nodes to key");

      if closest_nodes.is_empty() {
        let all_nodes = table.get_all_nodes();
        tracing::info!(
          node_count = all_nodes.len(),
          "No closest nodes found, fallback to known nodes"
        );
        all_nodes
      } else {
        closest_nodes
      }
    };

    tracing::info!(node_count = nodes.len(), "Attempting to store on nodes");

    if nodes.is_empty() {
      tracing::warn!("No known nodes to store value on. Store only locally.");
    } else {
      // Store on K closest nodes or all known nodes if we have fewer than K
      for node in nodes {
        if node.id != self.node_id {
          tracing::debug!(node_id = %node.id, "Storing value on node");
          // Longer timeout for reliability
          match tokio::time::timeout(Duration::from_secs(20), self.store(&node, key.clone(), value.clone())).await {
            Ok(Ok(response)) => match response {
              ResponseMessage::StoreResult { success, .. } => {
                tracing::debug!(
                  node_id = %node.id,
                  success = success,
                  "Store result from node"
                );
              }
              _ => tracing::warn!("Unexpected response from store operation"),
            },
            Ok(Err(e)) => tracing::warn!(node_id = %node.id, error = ?e, "Error storing on node"),
            Err(_) => tracing::warn!(node_id = %node.id, "Timeout storing on node"),
          }
        }
      }
    }

    Ok(())
  }

  async fn replicate_to_new_node(&self, node: &Node) -> Result<()> {
    let keys = {
      let storage = self.storage.read().await;
      storage.keys()
    };

    for key in keys {
      let should_store = {
        let table = self.routing_table.read().await;
        table.get_closest(&key, K).iter().any(|closest| closest.id == node.id)
      };

      if !should_store {
        continue;
      }

      let value_opt = {
        let mut storage = self.storage.write().await;
        storage.get(&key).ok()
      };

      if let Some(value) = value_opt {
        if let Err(err) = self.store(node, key.clone(), value).await {
          tracing::debug!(target = %node.id, key = %key, ?err, "Replication store failed");
        }
      }
    }

    Ok(())
  }

  pub(crate) async fn record_contact(&self, node: &Node) -> Result<bool> {
    if node.id == self.node_id {
      return Ok(false);
    }

    let (status, was_known) = {
      let mut table = self.routing_table.write().await;
      let known = table.contains(&node.id);
      let status = table.update(node.clone())?;
      (status, known)
    };

    let mut inserted = !was_known && matches!(status, UpdateStatus::Updated);

    if let UpdateStatus::PendingPing { node_to_ping } = status {
      match self.health_check_ping(&node_to_ping).await {
        Ok(()) => {
          let mut table = self.routing_table.write().await;
          table.node_seen(&node_to_ping.id)?;
        }
        Err(_) => {
          {
            let mut table = self.routing_table.write().await;
            table.remove(&node_to_ping.id);
          }
          let mut table = self.routing_table.write().await;
          if matches!(table.update(node.clone())?, UpdateStatus::Updated) && !was_known {
            inserted = true;
          }
        }
      }
    }

    Ok(inserted)
  }

  async fn health_check_ping(&self, node: &Node) -> Result<()> {
    let id = rand::random::<MessageId>();
    let local_node = Node::new(self.node_id.clone(), self.addr);
    let request = RequestMessage::Ping { id, sender: local_node };

    {
      let mut pending = self.pending_requests.lock().await;
      pending.insert(id, request.clone());
    }

    self.network.send(node.addr, KademliaMessage::Request(request)).await?;

    match timeout(RPC_TIMEOUT, self.network.wait_response(id, RPC_TIMEOUT)).await {
      Ok(result) => {
        let _ = result?;
        let mut pending = self.pending_requests.lock().await;
        pending.remove(&id);
        Ok(())
      }
      Err(_) => {
        let mut pending = self.pending_requests.lock().await;
        pending.remove(&id);
        Err(Error::Timeout)
      }
    }
  }
}
