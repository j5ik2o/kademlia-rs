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
use crate::routing::{K, RoutingTable};
use crate::storage::Storage;
use crate::{Error, Result};

/// Maximum concurrent RPCs during a node lookup
const ALPHA: usize = 3;

/// Default timeout for RPC operations
const RPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Protocol handler for Kademlia operations
pub struct Protocol<S, N>
where
    S: Storage,
    N: Network,
{
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
        // Update the routing table with the sender node
        let sender = req.sender().clone();
        {
            let mut table = self.routing_table.write().await;
            table.update(sender.clone())?;
        }

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
    async fn handle_find_node(
        &self,
        id: MessageId,
        _sender: Node,
        target: NodeId,
        from: SocketAddr,
    ) -> Result<()> {
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
    async fn handle_find_value(
        &self,
        id: MessageId,
        _sender: Node,
        key: NodeId,
        from: SocketAddr,
    ) -> Result<()> {
        // Check if we have the value locally
        let value_opt = {
            let storage = self.storage.read().await;
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
        
        let request = RequestMessage::Ping {
            id,
            sender: local_node,
        };
        
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, request.clone());
        }
        
        self.network.send(node.addr, KademliaMessage::Request(request)).await?;
        
        match timeout(RPC_TIMEOUT, self.network.wait_response(id, RPC_TIMEOUT)).await {
            Ok(result) => {
                let response = result?;
                
                // Update routing table with the responding node
                {
                    let mut table = self.routing_table.write().await;
                    table.update(response.sender().clone())?;
                }
                
                // Clean up pending request
                {
                    let mut pending = self.pending_requests.lock().await;
                    pending.remove(&id);
                }
                
                Ok(response)
            },
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
                
                // Update routing table with the responding node
                {
                    let mut table = self.routing_table.write().await;
                    table.update(response.sender().clone())?;
                }
                
                // Clean up pending request
                {
                    let mut pending = self.pending_requests.lock().await;
                    pending.remove(&id);
                }
                
                Ok(response)
            },
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
                    },
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
                
                // Update routing table with the responding node
                {
                    let mut table = self.routing_table.write().await;
                    table.update(response.sender().clone())?;
                }
                
                // Clean up pending request
                {
                    let mut pending = self.pending_requests.lock().await;
                    pending.remove(&id);
                }
                
                match response {
                    ResponseMessage::NodesFound { nodes, .. } => Ok(nodes),
                    _ => Err(Error::Other("Unexpected response type".to_string())),
                }
            },
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
        // First check if we have the value locally
        {
            let storage = self.storage.read().await;
            if let Ok(value) = storage.get(key) {
                println!("Value found in local node storage");
                return Ok(Some(value));
            }
        }

        // Local value not found, now we'll check other nodes

        // If not found locally, check other nodes
        // This is a simplification - in a real Kademlia DHT, we would use the iterative find algorithm
        let table = self.routing_table.read().await;
        let nodes = table.get_all_nodes();

        println!("Value not found locally, checking {} other nodes", nodes.len());

        if nodes.is_empty() {
            println!("Warning: No known nodes to query. Make sure you've joined the network.");
            return Ok(None);
        }

        // Try to get the value from all known nodes
        for node in &nodes {
            if node.id != self.node_id {
                println!("Checking for value on node: {}", node.id);

                // Send a FIND_VALUE request to the node with timeout
                match tokio::time::timeout(Duration::from_secs(2), self.find_value_rpc(&node, key.clone())).await {
                    Ok(find_result) => match find_result {
                        Ok((Some(value), _)) => {
                            println!("Found value on node: {}", node.id);

                            // Store the value locally for caching
                            {
                                let mut storage = self.storage.write().await;
                                let _ = storage.store(key, value.clone());
                            }

                            println!("Value retrieved from remote node: {}", node.id);

                            return Ok(Some(value));
                        },
                        Ok((None, closest_nodes)) => {
                            println!("Node {} returned {} closest nodes instead of value", node.id, closest_nodes.len());

                            // Check these closest nodes as well
                            let checked_ids: Vec<NodeId> = nodes.iter().map(|n| n.id.clone()).collect();
                            for closest in closest_nodes {
                                if closest.id != self.node_id && !checked_ids.contains(&closest.id) {
                                    println!("Checking closest node: {}", closest.id);
                                    // Add timeout to closest node query as well
                                    match tokio::time::timeout(Duration::from_secs(2), self.find_value_rpc(&closest, key.clone())).await {
                                        Ok(closest_result) => match closest_result {
                                            Ok((Some(value), _)) => {
                                                println!("Found value on closest node: {}", closest.id);

                                                // Cache the value locally
                                                {
                                                    let mut storage = self.storage.write().await;
                                                    let _ = storage.store(key, value.clone());
                                                }

                                                return Ok(Some(value));
                                            },
                                            _ => println!("Value not found on closest node: {}", closest.id)
                                        },
                                        Err(_) => println!("Timeout querying closest node {}", closest.id)
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            println!("Error querying node {}: {:?}", node.id, e);
                        }
                    },
                    Err(_) => println!("Timeout querying node {}", node.id)
                }
            }
        }

        // Not found anywhere
        println!("Value not found in the network");
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
        
        match timeout(RPC_TIMEOUT, self.network.wait_response(id, RPC_TIMEOUT)).await {
            Ok(result) => {
                let response = result?;
                
                // Update routing table with the responding node
                {
                    let mut table = self.routing_table.write().await;
                    table.update(response.sender().clone())?;
                }
                
                // Clean up pending request
                {
                    let mut pending = self.pending_requests.lock().await;
                    pending.remove(&id);
                }
                
                match response {
                    ResponseMessage::ValueFound { value, nodes, .. } => Ok((value, nodes)),
                    _ => Err(Error::Other("Unexpected response type".to_string())),
                }
            },
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

    /// Store a key-value pair in the network
    pub async fn store_value(&self, key: NodeId, value: Vec<u8>) -> Result<()> {
        // Store the value locally
        {
            let mut storage = self.storage.write().await;
            storage.store(&key, value.clone())?;
        }

        println!("Stored value locally for key {}", key);

        // Always store on bootstrap node if we have joined
        let table = self.routing_table.read().await;
        let nodes = table.get_all_nodes();

        println!("Attempting to store on {} known nodes", nodes.len());

        if nodes.is_empty() {
            println!("Warning: No known nodes to store value on. Store only locally.");
        } else {
            // Store on all known nodes (in a real implementation this would be more selective)
            for node in nodes {
                if node.id != self.node_id {
                    println!("Storing value on node: {}", node.id);
                    // Add a timeout to each store operation to prevent hanging
                    match tokio::time::timeout(Duration::from_secs(2), self.store(&node, key.clone(), value.clone())).await {
                        Ok(Ok(response)) => {
                            match response {
                                ResponseMessage::StoreResult { success, .. } => {
                                    println!("Store result from node {}: {}", node.id, if success { "SUCCESS" } else { "FAILED" });
                                },
                                _ => println!("Unexpected response from store operation")
                            }
                        },
                        Ok(Err(e)) => println!("Error storing on node {}: {:?}", node.id, e),
                        Err(_) => println!("Timeout storing on node {}", node.id)
                    }
                }
            }
        }

        Ok(())
    }
}