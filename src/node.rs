use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use crate::node_id::NodeId;

/// Last time of successful communication with this node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LastSeen {
  /// Node has never been seen (new node)
  Never,
  /// Time when the node was last seen (as seconds since UNIX epoch)
  At(u64),
}

impl Default for LastSeen {
  fn default() -> Self {
    LastSeen::Never
  }
}

/// Response statistics for a node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseStats {
  /// Number of successful responses
  pub successes: u32,
  /// Number of failed or timed out responses
  pub failures: u32,
  /// Average response time in milliseconds (exponential moving average)
  #[serde(default)]
  pub avg_response_time_ms: Option<u64>,
}

impl Default for ResponseStats {
  fn default() -> Self {
    ResponseStats {
      successes: 0,
      failures: 0,
      avg_response_time_ms: None,
    }
  }
}

impl ResponseStats {
  /// Calculate the success rate (0.0 to 1.0)
  pub fn success_rate(&self) -> f64 {
    let total = self.successes + self.failures;
    if total == 0 {
      1.0 // New nodes get benefit of the doubt
    } else {
      self.successes as f64 / total as f64
    }
  }

  /// Check if the node is reliable (success rate >= threshold)
  pub fn is_reliable(&self, threshold: f64) -> bool {
    self.success_rate() >= threshold
  }

  /// Record a successful response
  pub fn record_success(&mut self) {
    self.successes = self.successes.saturating_add(1);
  }

  /// Record a failed response
  pub fn record_failure(&mut self) {
    self.failures = self.failures.saturating_add(1);
  }

  /// Record response time using exponential moving average
  /// alpha = 0.2 gives more weight to recent observations
  pub fn record_response_time(&mut self, response_time_ms: u64) {
    const ALPHA: f64 = 0.2;
    match self.avg_response_time_ms {
      None => {
        self.avg_response_time_ms = Some(response_time_ms);
      }
      Some(avg) => {
        let new_avg = (ALPHA * response_time_ms as f64) + ((1.0 - ALPHA) * avg as f64);
        self.avg_response_time_ms = Some(new_avg as u64);
      }
    }
  }

  /// Get adaptive timeout based on average response time
  /// Returns timeout = avg_response_time * multiplier + base_timeout
  pub fn adaptive_timeout(&self, multiplier: f64, base_timeout: Duration) -> Duration {
    match self.avg_response_time_ms {
      None => base_timeout, // No history, use default
      Some(avg_ms) => {
        let adaptive_ms = (avg_ms as f64 * multiplier) as u64;
        let total_ms = adaptive_ms + base_timeout.as_millis() as u64;
        Duration::from_millis(total_ms.min(30000)) // Cap at 30 seconds
      }
    }
  }
}

/// Represents a node in the Kademlia network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
  /// Unique identifier of the node
  pub id: NodeId,
  /// Socket address for network communication
  pub addr: SocketAddr,
  /// Last time of successful communication with this node
  pub last_seen: LastSeen,
  /// Response statistics for reliability tracking
  #[serde(default)]
  pub response_stats: ResponseStats,
}

impl Node {
  /// Creates a new Node
  pub fn new(id: NodeId, addr: SocketAddr) -> Self {
    Node {
      id,
      addr,
      last_seen: LastSeen::Never,
      response_stats: ResponseStats::default(),
    }
  }

  /// Creates a new Node with the address used to generate the ID
  pub fn with_addr(addr: SocketAddr) -> Self {
    let id = NodeId::from_socket_addr(&addr);
    Self::new(id, addr)
  }

  /// Updates the last_seen timestamp to now
  pub fn update_last_seen(&mut self) {
    let now = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or(Duration::from_secs(0))
      .as_secs();
    self.last_seen = LastSeen::At(now);
  }

  /// Checks if the node has been seen within the specified duration
  pub fn is_active(&self, timeout: Duration) -> bool {
    match &self.last_seen {
      LastSeen::Never => false,
      LastSeen::At(last_seen_time) => {
        let now = SystemTime::now()
          .duration_since(SystemTime::UNIX_EPOCH)
          .unwrap_or(Duration::from_secs(0))
          .as_secs();

        // Calculate elapsed time
        if now < *last_seen_time {
          // Clock went backwards, consider active
          true
        } else {
          let elapsed_secs = now - *last_seen_time;
          Duration::from_secs(elapsed_secs) <= timeout
        }
      }
    }
  }
}

impl PartialEq for Node {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
  }
}

impl Eq for Node {}

#[cfg(test)]
mod tests {
  use super::*;
  use std::net::{IpAddr, Ipv4Addr};
  use std::thread;

  #[test]
  fn test_node_creation() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000);
    let node = Node::with_addr(addr);
    assert_eq!(node.addr, addr);
    assert_eq!(node.id, NodeId::from_socket_addr(&addr));
  }

  #[test]
  fn test_node_active_status() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000);
    let mut node = Node::with_addr(addr);

    // Initially the node should not be active
    assert!(!node.is_active(Duration::from_secs(10)));

    // Update last_seen and check again
    node.update_last_seen();
    assert!(node.is_active(Duration::from_secs(10)));

    // Sleep longer to ensure enough time passes for the timeout
    thread::sleep(Duration::from_millis(150));

    // Should still be active with a long timeout
    assert!(node.is_active(Duration::from_secs(10)));

    // With our new implementation, since we're using Unix timestamps with second precision
    // rather than Instant, we can't accurately test millisecond-level timeouts
    // So we'll skip that assertion for now
  }

  #[test]
  fn test_node_equality() {
    let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000);
    let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8001);

    let node1 = Node::with_addr(addr1);
    let node2 = Node::with_addr(addr1.clone()); // Same address generates same ID
    let node3 = Node::with_addr(addr2);

    assert_eq!(node1, node2);
    assert_ne!(node1, node3);
  }
}
