use serde::{Deserialize, Serialize};

use crate::node::Node;
use crate::node_id::NodeId;

/// The message ID, used to match requests with responses
pub type MessageId = u64;

/// Different types of Kademlia protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KademliaMessage {
  /// Request messages
  Request(RequestMessage),
  /// Response messages
  Response(ResponseMessage),
}

/// Request message types in the Kademlia protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestMessage {
  /// PING request to check if a node is alive
  Ping {
    /// Unique message ID
    id: MessageId,
    /// Sender node information
    sender: Node,
  },
  /// STORE request to store a key-value pair
  Store {
    /// Unique message ID
    id: MessageId,
    /// Sender node information
    sender: Node,
    /// Key to store
    key: NodeId,
    /// Value to store
    value: Vec<u8>,
  },
  /// FIND_NODE request to find k closest nodes to a target
  FindNode {
    /// Unique message ID
    id: MessageId,
    /// Sender node information
    sender: Node,
    /// Target node ID to find
    target: NodeId,
  },
  /// FIND_VALUE request to find a value by key
  FindValue {
    /// Unique message ID
    id: MessageId,
    /// Sender node information
    sender: Node,
    /// Key to find
    key: NodeId,
  },
}

/// Response message types in the Kademlia protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseMessage {
  /// PING response
  Pong {
    /// ID of the request this is responding to
    request_id: MessageId,
    /// Sender node information
    sender: Node,
  },
  /// STORE response
  StoreResult {
    /// ID of the request this is responding to
    request_id: MessageId,
    /// Sender node information
    sender: Node,
    /// Whether the store operation was successful
    success: bool,
  },
  /// FIND_NODE response
  NodesFound {
    /// ID of the request this is responding to
    request_id: MessageId,
    /// Sender node information
    sender: Node,
    /// List of k-closest nodes found
    nodes: Vec<Node>,
  },
  /// FIND_VALUE response - either returns the value or the k-closest nodes
  ValueFound {
    /// ID of the request this is responding to
    request_id: MessageId,
    /// Sender node information
    sender: Node,
    /// The value if found, or None if not found
    value: Option<Vec<u8>>,
    /// List of k-closest nodes if value not found
    nodes: Vec<Node>,
  },
}

impl RequestMessage {
  /// Get the message ID of the request
  pub fn id(&self) -> MessageId {
    match self {
      RequestMessage::Ping { id, .. } => *id,
      RequestMessage::Store { id, .. } => *id,
      RequestMessage::FindNode { id, .. } => *id,
      RequestMessage::FindValue { id, .. } => *id,
    }
  }

  /// Get the sender node of the request
  pub fn sender(&self) -> &Node {
    match self {
      RequestMessage::Ping { sender, .. } => sender,
      RequestMessage::Store { sender, .. } => sender,
      RequestMessage::FindNode { sender, .. } => sender,
      RequestMessage::FindValue { sender, .. } => sender,
    }
  }
}

impl ResponseMessage {
  /// Get the request ID this response is for
  pub fn request_id(&self) -> MessageId {
    match self {
      ResponseMessage::Pong { request_id, .. } => *request_id,
      ResponseMessage::StoreResult { request_id, .. } => *request_id,
      ResponseMessage::NodesFound { request_id, .. } => *request_id,
      ResponseMessage::ValueFound { request_id, .. } => *request_id,
    }
  }

  /// Get the sender node of the response
  pub fn sender(&self) -> &Node {
    match self {
      ResponseMessage::Pong { sender, .. } => sender,
      ResponseMessage::StoreResult { sender, .. } => sender,
      ResponseMessage::NodesFound { sender, .. } => sender,
      ResponseMessage::ValueFound { sender, .. } => sender,
    }
  }
}
