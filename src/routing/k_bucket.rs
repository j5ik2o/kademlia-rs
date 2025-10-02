use std::collections::VecDeque;
use std::time::Duration;

use crate::node::Node;
use crate::node_id::NodeId;
use crate::{Error, Result};

/// The default k parameter for k-buckets (maximum number of nodes per bucket)
pub const K: usize = 20;

/// The default timeout for considering a node as active
pub const NODE_TIMEOUT: Duration = Duration::from_secs(60 * 15); // 15 minutes

/// Minimum success rate threshold for keeping a node in the bucket
pub const MIN_SUCCESS_RATE: f64 = 0.5; // 50% success rate

#[derive(Clone)]
pub enum KBucketUpdate {
  Unchanged,
  Updated,
  PendingPing(Node),
}

/// A k-bucket stores up to k contacts of nodes close to a given portion of the key space
pub struct KBucket {
  /// Maximum number of nodes in the bucket
  k: usize,
  /// The NodeId of the local node this bucket belongs to
  local_node_id: NodeId,
  /// Prefix length this bucket is responsible for
  prefix_len: usize,
  /// Queue of nodes in order of least recently seen
  nodes: VecDeque<Node>,
  /// Cache of nodes to replace inactive nodes in the k-bucket
  replacement_cache: VecDeque<Node>,
}

impl KBucket {
  /// Creates a new empty k-bucket
  pub fn new(local_node_id: NodeId, prefix_len: usize, k: usize) -> Self {
    KBucket {
      k,
      local_node_id,
      prefix_len,
      nodes: VecDeque::with_capacity(k),
      replacement_cache: VecDeque::with_capacity(k),
    }
  }

  /// Returns the number of nodes in the bucket
  pub fn len(&self) -> usize {
    self.nodes.len()
  }

  /// Returns whether the bucket is empty
  pub fn is_empty(&self) -> bool {
    self.nodes.is_empty()
  }

  /// Returns whether the bucket is full
  pub fn is_full(&self) -> bool {
    self.nodes.len() >= self.k
  }

  /// Returns the nodes in the bucket as a slice
  pub fn nodes(&self) -> Vec<&Node> {
    self.nodes.iter().collect()
  }

  /// Checks if a node with the given ID is in the bucket
  pub fn contains(&self, node_id: &NodeId) -> bool {
    self.nodes.iter().any(|node| &node.id == node_id)
  }

  /// Gets a reference to a node with the given ID, if it exists
  pub fn get(&self, node_id: &NodeId) -> Option<&Node> {
    self.nodes.iter().find(|node| &node.id == node_id)
  }

  /// Adds a node to the bucket, or moves it to the back if it already exists
  pub fn update(&mut self, mut node: Node) -> Result<KBucketUpdate> {
    // Don't add the local node to any bucket
    if node.id == self.local_node_id {
      return Err(Error::Other("Cannot add local node to k-bucket".to_string()));
    }

    // Check if the node is already in the bucket
    if let Some(pos) = self.nodes.iter().position(|n| n.id == node.id) {
      // Remove it and put it at the back (most recently seen)
      let mut existing = self.nodes.remove(pos).unwrap();
      existing.update_last_seen();
      self.nodes.push_back(existing);
      Ok(KBucketUpdate::Updated)
    } else if self.nodes.len() < self.k {
      // If the bucket is not full, add the node at the back
      node.update_last_seen();
      self.nodes.push_back(node);
      Ok(KBucketUpdate::Updated)
    } else {
      // If the bucket is full, check if any node is unreliable or inactive
      // Priority: unreliable nodes > inactive nodes > LRU nodes

      // First, try to find an unreliable node (success rate < threshold)
      if let Some(pos) = self
        .nodes
        .iter()
        .position(|n| !n.response_stats.is_reliable(MIN_SUCCESS_RATE))
      {
        let unreliable_node = self.nodes.remove(pos).unwrap();
        node.update_last_seen();
        self.nodes.push_back(node);
        tracing::info!(
          removed_node = %unreliable_node.id,
          success_rate = unreliable_node.response_stats.success_rate(),
          "Removed unreliable node from k-bucket"
        );
        return Ok(KBucketUpdate::Updated);
      }

      // Second, check if the least-recently seen node is inactive
      let front_node = self.nodes.front().cloned().expect("bucket not empty");
      if !front_node.is_active(NODE_TIMEOUT) {
        // If the least-recently seen node is inactive, remove it and add the new node
        self.nodes.pop_front();
        node.update_last_seen();
        self.nodes.push_back(node);
        Ok(KBucketUpdate::Updated)
      } else {
        // Otherwise, add to the replacement cache
        // Remove any existing entry for this node in the cache
        if let Some(pos) = self.replacement_cache.iter().position(|n| n.id == node.id) {
          self.replacement_cache.remove(pos);
        }

        // Add to the replacement cache (used when a node becomes inactive)
        node.update_last_seen();
        self.replacement_cache.push_back(node);
        Ok(KBucketUpdate::PendingPing(front_node))
      }
    }
  }

  /// Removes a node with the given ID from the bucket, if it exists
  pub fn remove(&mut self, node_id: &NodeId) -> Option<Node> {
    let pos = self.nodes.iter().position(|node| &node.id == node_id)?;
    let removed_node = self.nodes.remove(pos).unwrap();

    // If there's a node in the replacement cache, move it to the main bucket
    if let Some(replacement) = self.replacement_cache.pop_front() {
      let mut node = replacement;
      node.update_last_seen();
      self.nodes.push_back(node);
    }

    Some(removed_node)
  }

  /// Gets the k closest nodes to the given target ID
  pub fn get_closest(&self, target: &NodeId, count: usize) -> Vec<Node> {
    let mut nodes: Vec<_> = self.nodes.iter().cloned().collect();

    // Sort by XOR distance to the target
    nodes.sort_by(|a, b| {
      let dist_a = a.id.distance(target);
      let dist_b = b.id.distance(target);
      dist_a.cmp(&dist_b)
    });

    // Take at most `count` nodes
    nodes.truncate(count);
    nodes
  }

  /// Splits the bucket into two if it is full and contains node IDs with
  /// different bits at the prefix_len position
  pub fn should_split(&self) -> bool {
    if !self.is_full() {
      return false;
    }

    // Check if all nodes have the same prefix up to prefix_len + 1
    let bit_at_pos = self.nodes[0].id.bit_at(self.prefix_len);
    !self
      .nodes
      .iter()
      .all(|node| node.id.bit_at(self.prefix_len) == bit_at_pos)
  }

  /// Splits the bucket into two new buckets
  ///
  /// Returns (bucket with 0 at position prefix_len, bucket with 1 at position prefix_len)
  pub fn split(&self) -> (KBucket, KBucket) {
    let new_prefix_len = self.prefix_len + 1;

    let mut bucket0 = KBucket::new(self.local_node_id.clone(), new_prefix_len, self.k);
    let mut bucket1 = KBucket::new(self.local_node_id.clone(), new_prefix_len, self.k);

    for node in &self.nodes {
      let bit_at_pos = node.id.bit_at(self.prefix_len);
      let bucket = if bit_at_pos { &mut bucket1 } else { &mut bucket0 };
      let node_clone = node.clone();
      bucket.update(node_clone).ok();
    }

    (bucket0, bucket1)
  }
}
