use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

pub const KEY_LENGTH_BYTES: usize = 20; // 160 bits (standard for Kademlia)
pub const KEY_LENGTH_BITS: usize = KEY_LENGTH_BYTES * 8;

/// NodeId represents a 160-bit identifier for nodes in the Kademlia network
#[derive(Clone, Eq, Serialize, Deserialize)]
pub struct NodeId {
  bytes: [u8; KEY_LENGTH_BYTES],
}

impl NodeId {
  /// Creates a new, random NodeId
  pub fn random() -> Self {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; KEY_LENGTH_BYTES];
    rng.fill(&mut bytes);
    NodeId { bytes }
  }

  /// Creates a NodeId from a socket address
  pub fn from_socket_addr(addr: &SocketAddr) -> Self {
    let addr_str = addr.to_string();
    Self::from_bytes(&Sha256::digest(addr_str.as_bytes())[..KEY_LENGTH_BYTES])
  }

  /// Creates a NodeId from raw bytes
  pub fn from_bytes(data: &[u8]) -> Self {
    let mut bytes = [0u8; KEY_LENGTH_BYTES];
    let len = std::cmp::min(data.len(), KEY_LENGTH_BYTES);
    bytes[..len].copy_from_slice(&data[..len]);

    // デバッグ: 変換前後のバイト配列を表示
    println!("DEBUG: NodeId::from_bytes - Input data: {:?}", data);
    println!("DEBUG: NodeId::from_bytes - Converted bytes: {:?}", bytes);
    println!("DEBUG: NodeId::from_bytes - Hex representation: {}", hex::encode(bytes));

    NodeId { bytes }
  }

  /// Creates a NodeId from a hexadecimal string
  pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
    let bytes = hex::decode(hex_str)?;
    Ok(Self::from_bytes(&bytes))
  }

  /// Returns the raw bytes of the NodeId
  pub fn as_bytes(&self) -> &[u8; KEY_LENGTH_BYTES] {
    &self.bytes
  }

  /// Converts the NodeId to a hexadecimal string
  pub fn to_hex(&self) -> String {
    hex::encode(self.bytes)
  }

  /// Calculates the XOR distance between two NodeIds
  pub fn distance(&self, other: &NodeId) -> NodeId {
    let mut result = [0u8; KEY_LENGTH_BYTES];
    for i in 0..KEY_LENGTH_BYTES {
      result[i] = self.bytes[i] ^ other.bytes[i];
    }
    NodeId { bytes: result }
  }

  /// Returns the bit at the specified position
  pub fn bit_at(&self, position: usize) -> bool {
    if position >= KEY_LENGTH_BITS {
      return false;
    }

    let byte_pos = position / 8;
    let bit_pos = 7 - (position % 8); // MSB first

    (self.bytes[byte_pos] & (1 << bit_pos)) != 0
  }

  /// Calculates the number of leading zeros in the binary representation of the NodeId
  pub fn leading_zeros(&self) -> usize {
    let mut count = 0;
    'outer: for byte in &self.bytes {
      for bit in (0..8).rev() {
        if (byte & (1 << bit)) != 0 {
          break 'outer;
        }
        count += 1;
      }
    }
    count
  }

  /// Calculates the longest common prefix length with another NodeId
  pub fn common_prefix_length(&self, other: &NodeId) -> usize {
    self.distance(other).leading_zeros()
  }

  /// Calculates the bucket index for a given NodeId (used for k-bucket routing)
  pub fn bucket_index(&self, other: &NodeId) -> Option<usize> {
    let distance = self.distance(other);

    // Skip if the distance is 0 (same node)
    if distance.bytes.iter().all(|&b| b == 0) {
      return None;
    }

    // Find the index of the first set bit
    for i in 0..KEY_LENGTH_BYTES {
      if distance.bytes[i] == 0 {
        continue;
      }

      // Find the first set bit in this byte
      let byte = distance.bytes[i];
      for j in 0..8 {
        let bit = 7 - j; // MSB first
        if (byte & (1 << bit)) != 0 {
          return Some(i * 8 + j);
        }
      }
    }

    // This should never happen if distance != 0
    None
  }
}

impl PartialEq for NodeId {
  fn eq(&self, other: &Self) -> bool {
    self.bytes == other.bytes
  }
}

impl Ord for NodeId {
  fn cmp(&self, other: &Self) -> Ordering {
    self.bytes.cmp(&other.bytes)
  }
}

impl PartialOrd for NodeId {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Hash for NodeId {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.bytes.hash(state);
  }
}

impl fmt::Debug for NodeId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "NodeId({})", self.to_hex())
  }
}

impl fmt::Display for NodeId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.to_hex())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_node_id_creation() {
    let id1 = NodeId::random();
    let id2 = NodeId::random();
    assert_ne!(id1, id2);

    let addr = "127.0.0.1:8000".parse::<SocketAddr>().unwrap();
    let id3 = NodeId::from_socket_addr(&addr);
    let id4 = NodeId::from_socket_addr(&addr);
    assert_eq!(id3, id4);
  }

  #[test]
  fn test_distance_calculation() {
    let id1 = NodeId::from_bytes(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let id2 = NodeId::from_bytes(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let distance = id1.distance(&id2);
    assert_eq!(distance.bytes[0], 3);
  }

  #[test]
  fn test_bucket_index() {
    let id1 = NodeId::from_bytes(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let id2 = NodeId::from_bytes(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let idx = id1.bucket_index(&id2).unwrap();

    // For 1 (00000001) XOR 3 (00000011) = 2 (00000010)
    // The first set bit is at position 6 (0-indexed from MSB)
    assert_eq!(idx, 6);
  }

  #[test]
  fn test_common_prefix_length() {
    let id1 = NodeId::from_bytes(&[0b10100000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let id2 = NodeId::from_bytes(&[0b10110000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(id1.common_prefix_length(&id2), 3);
  }

  #[test]
  fn test_bit_at() {
    let id = NodeId::from_bytes(&[0b10100000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(id.bit_at(0), true);
    assert_eq!(id.bit_at(1), false);
    assert_eq!(id.bit_at(2), true);
    assert_eq!(id.bit_at(3), false);
  }
}
