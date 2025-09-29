use std::collections::HashMap;

use crate::node::Node;
use crate::node_id::{NodeId, KEY_LENGTH_BITS};
use crate::routing::k_bucket::{KBucket, K};
use crate::Result;

/// The routing table for a Kademlia node, consisting of k-buckets
pub struct RoutingTable {
  /// The NodeId of the local node
  local_node_id: NodeId,
  /// The k-buckets, where buckets[i] is responsible for nodes with distance
  /// having the first set bit at position i
  buckets: HashMap<usize, KBucket>,
  /// The k parameter (max nodes per bucket)
  k: usize,
}

impl RoutingTable {
  /// Creates a new empty routing table
  pub fn new(local_node_id: NodeId) -> Self {
    RoutingTable {
      local_node_id,
      buckets: HashMap::new(),
      k: K,
    }
  }

  /// Creates a new empty routing table with a specified k value
  pub fn with_k(local_node_id: NodeId, k: usize) -> Self {
    RoutingTable {
      local_node_id,
      buckets: HashMap::new(),
      k,
    }
  }

  /// Returns the local node ID
  pub fn local_node_id(&self) -> &NodeId {
    &self.local_node_id
  }

  /// Returns the bucket indices currently stored in the table
  pub fn bucket_indices(&self) -> Vec<usize> {
    let mut indices: Vec<usize> = self.buckets.keys().cloned().collect();
    indices.sort_unstable();
    indices
  }

  /// Returns the number of nodes in the routing table
  pub fn node_count(&self) -> usize {
    self.buckets.values().map(|bucket| bucket.len()).sum()
  }

  /// Determines the bucket index for a given node ID
  fn bucket_index(&self, node_id: &NodeId) -> Option<usize> {
    self.local_node_id.bucket_index(node_id)
  }

  /// Gets or creates a bucket for the specified index
  fn get_or_create_bucket(&mut self, index: usize) -> &mut KBucket {
    if !self.buckets.contains_key(&index) {
      let bucket = KBucket::new(self.local_node_id.clone(), index, self.k);
      self.buckets.insert(index, bucket);
    }
    self.buckets.get_mut(&index).unwrap()
  }

  /// Adds a node to the routing table or updates its position if already present
  pub fn update(&mut self, node: Node) -> Result<()> {
    if node.id == self.local_node_id {
      return Ok(());
    }

    let index = match self.bucket_index(&node.id) {
      Some(idx) => idx,
      None => return Ok(()),
    };

    let bucket = self.get_or_create_bucket(index);
    let result = bucket.update(node.clone());

    // Check if the bucket should be split
    if bucket.should_split() && index < KEY_LENGTH_BITS - 1 {
      let (bucket0, bucket1) = bucket.split();
      self.buckets.remove(&index);
      self.buckets.insert(index, bucket0);
      self.buckets.insert(index + 1, bucket1);

      // Try adding the node again after the split
      return self.update(node);
    }

    result
  }

  /// Updates a node's last seen timestamp and position in its bucket
  pub fn node_seen(&mut self, node_id: &NodeId) -> Result<()> {
    if let Some(index) = self.bucket_index(node_id) {
      if let Some(bucket) = self.buckets.get_mut(&index) {
        if let Some(node) = bucket.get(node_id) {
          // Clone the node, update it, and call update
          let mut node_clone = node.clone();
          node_clone.update_last_seen();
          return bucket.update(node_clone);
        }
      }
    }
    Ok(())
  }

  /// Removes a node from the routing table
  pub fn remove(&mut self, node_id: &NodeId) -> Option<Node> {
    let index = self.bucket_index(node_id)?;
    let bucket = self.buckets.get_mut(&index)?;
    bucket.remove(node_id)
  }

  /// Checks if a node with the given ID is in the routing table
  pub fn contains(&self, node_id: &NodeId) -> bool {
    if let Some(index) = self.bucket_index(node_id) {
      if let Some(bucket) = self.buckets.get(&index) {
        return bucket.contains(node_id);
      }
    }
    false
  }

  /// Gets a reference to a node with the given ID, if it exists in the routing table
  pub fn get(&self, node_id: &NodeId) -> Option<&Node> {
    let index = self.bucket_index(node_id)?;
    let bucket = self.buckets.get(&index)?;
    bucket.get(node_id)
  }

  /// Gets the k closest nodes to the given target ID
  pub fn get_closest(&self, target: &NodeId, count: usize) -> Vec<Node> {
    let mut all_nodes = Vec::new();

    // Collect nodes from all buckets
    for bucket in self.buckets.values() {
      all_nodes.extend(bucket.nodes().into_iter().cloned());
    }

    // Sort by XOR distance to the target
    all_nodes.sort_by(|a, b| {
      let dist_a = a.id.distance(target);
      let dist_b = b.id.distance(target);
      dist_a.cmp(&dist_b)
    });

    // Take at most `count` nodes
    all_nodes.truncate(count);
    all_nodes
  }

  /// Returns all nodes in the routing table
  pub fn get_all_nodes(&self) -> Vec<Node> {
    let mut all_nodes = Vec::new();

    for bucket in self.buckets.values() {
      for node in bucket.nodes() {
        all_nodes.push(node.clone());
      }
    }

    all_nodes
  }
}
