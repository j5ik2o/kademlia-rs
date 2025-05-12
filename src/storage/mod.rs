use std::collections::HashMap;
use std::time::{Duration, Instant};
use async_trait::async_trait;

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
}

impl MemoryStorage {
    /// Create a new empty memory storage
    pub fn new() -> Self {
        MemoryStorage {
            storage: HashMap::new(),
            default_ttl: DEFAULT_TTL,
        }
    }
    
    /// Create a new memory storage with a specific default TTL
    pub fn with_ttl(ttl: Duration) -> Self {
        MemoryStorage {
            storage: HashMap::new(),
            default_ttl: ttl,
        }
    }
    
    /// Clean up expired entries
    pub fn cleanup(&mut self) {
        let now = Instant::now();
        self.storage.retain(|_, entry| {
            entry.timestamp + entry.ttl > now
        });
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
        self.store_with_ttl(key, value, self.default_ttl)
    }
    
    fn get(&self, key: &NodeId) -> Result<Vec<u8>> {
        match self.storage.get(key) {
            Some(entry) => {
                let now = Instant::now();
                if entry.timestamp + entry.ttl > now {
                    Ok(entry.value.clone())
                } else {
                    Err(Error::ValueNotFound)
                }
            },
            None => Err(Error::ValueNotFound),
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