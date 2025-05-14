use async_trait::async_trait;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::node_id::NodeId;
use crate::{Error, Result};

/// Storage trait for key-value storage in Kademlia
#[async_trait]
pub trait Storage: Send + Sync + 'static {
  /// Store a key-value pair
  fn store(&mut self, key: &NodeId, value: Vec<u8>) -> Result<()>;

  /// Retrieve a value by key
  fn get(&self, key: &NodeId) -> Result<Vec<u8>>;

  /// Remove a key-value pair
  fn remove(&mut self, key: &NodeId) -> Result<()>;
}

/// Default time-to-live for stored values
const DEFAULT_TTL: Duration = Duration::from_secs(86400); // 24 hours

/// An entry in the memory storage
struct StorageEntry {
  /// The stored value
  value: Vec<u8>,
  /// When this entry was last updated
  timestamp: Instant,
  /// Time-to-live for this entry
  ttl: Duration,
}

/// In-memory implementation of the Storage trait
pub struct MemoryStorage {
  /// Map of keys to storage entries
  storage: HashMap<NodeId, StorageEntry>,
  /// Default TTL for entries
  default_ttl: Duration,
  /// Debug name for this storage instance
  debug_name: String,
}

impl MemoryStorage {
  /// Create a new empty memory storage
  pub fn new() -> Self {
    MemoryStorage {
      storage: HashMap::new(),
      default_ttl: DEFAULT_TTL,
      debug_name: "default".to_string(),
    }
  }

  /// Create a new memory storage with a specific default TTL
  pub fn with_ttl(ttl: Duration) -> Self {
    MemoryStorage {
      storage: HashMap::new(),
      default_ttl: ttl,
      debug_name: "default".to_string(),
    }
  }

  /// Create a new memory storage with a specific name (for debugging)
  pub fn with_name(name: &str) -> Self {
    MemoryStorage {
      storage: HashMap::new(),
      default_ttl: DEFAULT_TTL,
      debug_name: name.to_string(),
    }
  }

  /// Create a new memory storage with both name and TTL
  pub fn with_name_and_ttl(name: &str, ttl: Duration) -> Self {
    MemoryStorage {
      storage: HashMap::new(),
      default_ttl: ttl,
      debug_name: name.to_string(),
    }
  }

  /// Get all stored keys
  pub fn get_all_keys(&self) -> Vec<NodeId> {
    self.storage.keys().cloned().collect()
  }

  /// Get iterator over all stored items
  pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &Vec<u8>)> {
    self.storage.iter().map(|(k, entry)| (k, &entry.value))
  }

  /// Debug method to dump all stored values
  pub fn dump_storage(&self) {
    println!("===== Storage dump for '{}' ({} items) =====", self.debug_name, self.storage.len());
    for (key, entry) in &self.storage {
      let value_preview = if entry.value.len() > 20 {
        format!("{:?}... ({} bytes)", &entry.value[0..20], entry.value.len())
      } else {
        format!("{:?}", entry.value)
      };

      println!("Key: {} -> Value: {}", key, value_preview);
    }
    println!("===== End of storage dump =====");
  }

  /// Clean up expired entries
  pub fn cleanup(&mut self) {
    let now = Instant::now();
    self.storage.retain(|_, entry| entry.timestamp + entry.ttl > now);
  }

  /// Store a key-value pair with a specific TTL
  pub fn store_with_ttl(&mut self, key: &NodeId, value: Vec<u8>, ttl: Duration) -> Result<()> {
    let entry = StorageEntry {
      value,
      timestamp: Instant::now(),
      ttl,
    };

    self.storage.insert(key.clone(), entry);
    Ok(())
  }
}

impl Storage for MemoryStorage {
  fn store(&mut self, key: &NodeId, value: Vec<u8>) -> Result<()> {
    println!("DEBUG: MemoryStorage({}): Storing value for key: {}", self.debug_name, key);
    let result = self.store_with_ttl(key, value, self.default_ttl);

    // ストア後にストレージの状態をダンプ（デバッグ用）
    if result.is_ok() {
      println!("DEBUG: MemoryStorage({}): Successfully stored value", self.debug_name);
      // 重要なところだけ詳細にダンプ
      if self.storage.len() < 10 {
        self.dump_storage();
      } else {
        println!("DEBUG: MemoryStorage({}): Storage contains {} items",
                 self.debug_name, self.storage.len());
      }
    }

    result
  }

  fn get(&self, key: &NodeId) -> Result<Vec<u8>> {
    println!("DEBUG: MemoryStorage({}): Looking up key: {}", self.debug_name, key);

    match self.storage.get(key) {
      Some(entry) => {
        let now = Instant::now();
        if entry.timestamp + entry.ttl > now {
          println!("DEBUG: MemoryStorage({}): Found value for key: {} (size: {} bytes)",
                   self.debug_name, key, entry.value.len());
          Ok(entry.value.clone())
        } else {
          println!("DEBUG: MemoryStorage({}): Key found but entry expired: {}",
                   self.debug_name, key);
          Err(Error::ValueNotFound)
        }
      }
      None => {
        println!("DEBUG: MemoryStorage({}): Key not found: {}", self.debug_name, key);

        // キーが見つからない場合は、既存の全キーをログに出力して確認
        if !self.storage.is_empty() {
          println!("DEBUG: MemoryStorage({}): Available keys:", self.debug_name);
          for stored_key in self.storage.keys() {
            println!("  - {}", stored_key);
          }
        }

        Err(Error::ValueNotFound)
      }
    }
  }

  fn remove(&mut self, key: &NodeId) -> Result<()> {
    if self.storage.remove(key).is_some() {
      Ok(())
    } else {
      Err(Error::ValueNotFound)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::thread;

  #[test]
  fn test_memory_storage() {
    let mut storage = MemoryStorage::new();
    let key = NodeId::random();
    let value = vec![1, 2, 3, 4];

    // Test store and get
    storage.store(&key, value.clone()).unwrap();
    assert_eq!(storage.get(&key).unwrap(), value);

    // Test remove
    storage.remove(&key).unwrap();
    assert!(storage.get(&key).is_err());
  }

  #[test]
  fn test_storage_ttl() {
    let mut storage = MemoryStorage::with_ttl(Duration::from_millis(100));
    let key = NodeId::random();
    let value = vec![1, 2, 3, 4];

    storage.store(&key, value.clone()).unwrap();
    assert_eq!(storage.get(&key).unwrap(), value);

    // Sleep to let the entry expire
    thread::sleep(Duration::from_millis(150));

    // The get should fail now
    assert!(storage.get(&key).is_err());
  }
}
