use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::node_id::NodeId;
use crate::{Error, Result};

/// Storage trait for key-value storage in Kademlia
#[async_trait]
pub trait Storage: Send + Sync + 'static {
  /// Store a key-value pair
  fn store(&mut self, key: &NodeId, value: Vec<u8>) -> Result<()>;

  /// Retrieve a value by key
  fn get(&mut self, key: &NodeId) -> Result<Vec<u8>>;

  /// Remove a key-value pair
  fn remove(&mut self, key: &NodeId) -> Result<()>;
}

/// Default time-to-live for stored values
const DEFAULT_TTL: Duration = Duration::from_secs(86400); // 24 hours

/// An entry in the memory storage
#[derive(Clone)]
struct StorageEntry {
  /// The stored value
  value: Vec<u8>,
  /// When this entry was last updated
  timestamp: Instant,
  /// Time-to-live for this entry
  ttl: Duration,
}

impl Default for StorageEntry {
  fn default() -> Self {
    StorageEntry {
      value: Vec::new(),
      timestamp: Instant::now(),
      ttl: DEFAULT_TTL,
    }
  }
}

/// In-memory implementation of the Storage trait
#[derive(Default)]
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
    tracing::debug!(
      storage_name = %self.debug_name,
      items_count = self.storage.len(),
      "===== Storage dump start ====="
    );

    for (key, entry) in &self.storage {
      let value_preview = if entry.value.len() > 20 {
        format!("{:?}... ({} bytes)", &entry.value[0..20], entry.value.len())
      } else {
        format!("{:?}", entry.value)
      };

      tracing::debug!(key = %key, value = %value_preview, "Storage entry");
    }

    tracing::debug!(storage_name = %self.debug_name, "===== Storage dump end =====");
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

  /// Create a new shared memory storage backed by this instance
  pub fn create_shared(&self) -> (Arc<tokio::sync::Mutex<HashMap<NodeId, Vec<u8>>>>, String) {
    let shared_storage = Arc::new(tokio::sync::Mutex::new(HashMap::<NodeId, Vec<u8>>::new()));
    let debug_name = self.debug_name.clone();

    // Clone the storage data for initial state
    for (key, entry) in &self.storage {
      let key_clone = key.clone();
      let value_clone = entry.value.clone();
      // Execute synchronously to avoid async complications
      tokio::task::block_in_place(|| {
        let mut storage = futures::executor::block_on(shared_storage.lock());
        storage.insert(key_clone, value_clone);
      });
    }

    (shared_storage, debug_name)
  }
}

impl Storage for MemoryStorage {
  fn store(&mut self, key: &NodeId, value: Vec<u8>) -> Result<()> {
    tracing::debug!(storage_name = %self.debug_name, key = %key, "Storing value for key");
    let result = self.store_with_ttl(key, value, self.default_ttl);

    // Dump storage state after store operation (for debugging)
    if result.is_ok() {
      tracing::debug!(storage_name = %self.debug_name, "Successfully stored value");
      // Only dump details for important items
      if self.storage.len() < 10 {
        self.dump_storage();
      } else {
        tracing::debug!(
          storage_name = %self.debug_name,
          items_count = self.storage.len(),
          "Storage contains items"
        );
      }
    }

    result
  }

  fn get(&mut self, key: &NodeId) -> Result<Vec<u8>> {
    tracing::debug!(storage_name = %self.debug_name, key = %key, "Looking up key");

    match self.storage.get(key) {
      Some(entry) => {
        let now = Instant::now();
        if entry.timestamp + entry.ttl > now {
          tracing::debug!(
            storage_name = %self.debug_name,
            key = %key,
            value_size = entry.value.len(),
            "Found value for key"
          );
          Ok(entry.value.clone())
        } else {
          tracing::debug!(storage_name = %self.debug_name, key = %key, "Key found but entry expired");
          Err(Error::ValueNotFound)
        }
      }
      None => {
        tracing::debug!(storage_name = %self.debug_name, key = %key, "Key not found");

        // If key is not found, log all existing keys for verification
        if !self.storage.is_empty() {
          tracing::debug!(storage_name = %self.debug_name, "Available keys:");
          for stored_key in self.storage.keys() {
            tracing::trace!("  - {}", stored_key);
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

/// DualStorage provides synchronized access to both a MemoryStorage instance
/// and a shared HashMap used by the UdpNetwork for coordination.
pub struct DualStorage {
  /// Local storage with full functionality
  memory_storage: MemoryStorage,
  /// Shared storage for network coordination
  shared_storage: Arc<tokio::sync::Mutex<HashMap<NodeId, Vec<u8>>>>,
}

impl DualStorage {
  /// Create a new DualStorage with a specific name
  pub fn new(name: &str) -> Self {
    let memory_storage = MemoryStorage::with_name(name);
    let (shared_storage, _) = memory_storage.create_shared();

    DualStorage {
      memory_storage,
      shared_storage,
    }
  }

  /// Get the shared storage for UdpNetwork
  pub fn shared_storage(&self) -> Arc<tokio::sync::Mutex<HashMap<NodeId, Vec<u8>>>> {
    self.shared_storage.clone()
  }

  /// Dump storage contents for debugging
  pub fn dump_storage(&self) {
    self.memory_storage.dump_storage();
  }
}

impl Storage for DualStorage {
  fn store(&mut self, key: &NodeId, value: Vec<u8>) -> Result<()> {
    // Store in memory storage first
    let result = self.memory_storage.store(key, value.clone());

    // Then ensure it's in the shared storage
    tokio::task::block_in_place(|| {
      let mut storage = futures::executor::block_on(self.shared_storage.lock());
      storage.insert(key.clone(), value);
    });

    result
  }

  fn get(&mut self, key: &NodeId) -> Result<Vec<u8>> {
    // First try memory storage for faster access
    match self.memory_storage.get(key) {
      Ok(value) => Ok(value),
      Err(_) => {
        // If not found, check the shared storage
        let value_opt = tokio::task::block_in_place(|| {
          let storage = futures::executor::block_on(self.shared_storage.lock());
          storage.get(key).cloned()
        });

        match value_opt {
          Some(value) => {
            // Also update memory storage for future faster lookups
            tracing::debug!(
              storage_name = %self.memory_storage.debug_name,
              key = %key,
              "Value found in shared storage but not in memory, syncing"
            );
            // Store directly since we have a mutable self
            let _ = self.memory_storage.store(key, value.clone());
            Ok(value)
          }
          None => Err(Error::ValueNotFound),
        }
      }
    }
  }

  fn remove(&mut self, key: &NodeId) -> Result<()> {
    // Remove from both storages
    let result = self.memory_storage.remove(key);

    tokio::task::block_in_place(|| {
      let mut storage = futures::executor::block_on(self.shared_storage.lock());
      storage.remove(key);
    });

    result
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
