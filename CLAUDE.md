# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Building and Running

```bash
# Build the library
cargo build

# Run tests
cargo test

# Run a specific test
cargo test test_store_and_retrieve

# Run with logging enabled
RUST_LOG=debug cargo test

# Run a simple bootstrap node (on port 8000)
cargo run --example simple_node -- --port 8000 bootstrap

# Join an existing network
cargo run --example simple_node -- --port 8001 join --bootstrap 127.0.0.1:8000

# Store a key-value pair
cargo run --example simple_node -- --port 8002 store --bootstrap 127.0.0.1:8000 --key mykey --value myvalue

# Retrieve a value by key
cargo run --example simple_node -- --port 8003 get --bootstrap 127.0.0.1:8000 --key mykey

# Run the DHT test script
./test_dht.sh
```

## Architecture

This Rust implementation of the Kademlia distributed hash table is organized into several modules:

### Core Components

1. **NodeID (`src/node_id/mod.rs`)**
   - 160-bit node identifier used for uniquely identifying nodes
   - Implements XOR-based distance metric for node comparisons
   - Critical for determining proximity in the network

2. **Node (`src/node/mod.rs`)**
   - Represents a contact in the Kademlia network
   - Contains node ID and network address information
   - Foundation for peer-to-peer communication

3. **Routing Table (`src/routing/routing_table.rs`)**
   - Manages k-buckets for efficient node lookup
   - Keeps track of known nodes based on their distance
   - Implements bucket splitting and node replacement strategies

4. **K-Bucket (`src/routing/k_bucket.rs`)**
   - Stores up to k contacts in a specific distance range
   - Implements least-recently-seen eviction policy
   - Handles node updates and replacements

5. **Protocol (`src/protocol/mod.rs`)**
   - Defines the Kademlia protocol operations and messages
   - Implements PING, STORE, FIND_NODE, and FIND_VALUE operations
   - Handles message serialization and deserialization

6. **Network (`src/network/mod.rs`, `src/network/udp.rs`, `src/network/node.rs`)**
   - Implements UDP-based network communication
   - Manages sending and receiving messages between nodes
   - Handles node bootstrapping and joining the network

7. **Storage (`src/storage/mod.rs`)**
   - Provides key-value storage functionality
   - Implements in-memory storage with TTL support

8. **Error Handling (`src/error/mod.rs`)**
   - Defines error types for the entire library
   - Provides clear error messages and handling mechanisms

### Data Flow

1. When a node starts, it creates a random NodeID and initializes its routing table.
2. To join a network, a node connects to a bootstrap node and performs a FIND_NODE operation for its own ID.
3. The node populates its routing table with contacts from the bootstrap node's response.
4. For STORE operations, the node locates the k closest nodes to the key and sends STORE messages to them.
5. For FIND_VALUE operations, the node recursively asks nodes that are closer to the key until it finds the value or determines it doesn't exist.

### UDP Communication

- The implementation uses asynchronous UDP sockets for communication between nodes.
- Messages are serialized using bincode for efficient transmission.
- Timeout handling is implemented to deal with network delays and node failures.

## Implementation Notes

1. This implementation is a research/educational project and may not be production-ready.

2. The current implementation uses in-memory storage which is not persistent across restarts.

3. The codebase uses Tokio for asynchronous runtime and relies heavily on async/await patterns.

4. When testing with multiple nodes, ensure unique port numbers for each node.

5. UDP communication doesn't guarantee message delivery - the implementation handles retries and timeouts.

6. Bootstrap nodes play a crucial role - they must be running before other nodes can join the network.

7. The implementation follows the original Kademlia paper's approach with k-buckets and XOR distance metrics.