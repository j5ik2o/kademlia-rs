# Kademlia DHT Implementation in Rust

This is a Rust implementation of the [Kademlia distributed hash table](https://en.wikipedia.org/wiki/Kademlia), a peer-to-peer distributed hash table designed for decentralized key-value storage with efficient lookup operations.

## Features

- Complete implementation of the Kademlia protocol
- XOR-based distance metric for node ID comparison
- K-bucket routing table with configurable k parameter
- Four basic operations: PING, STORE, FIND_NODE, FIND_VALUE
- UDP-based network communication
- In-memory key-value storage
- Support for time-to-live (TTL) for stored values
- Background tasks for routing table maintenance
- Simple CLI for testing and demonstration

## Architecture

The implementation is organized into several modules:

- `node_id`: Implements the 160-bit NodeId and distance calculations
- `node`: Defines the contact structure for remote nodes
- `routing`: Implements the k-bucket routing table
- `protocol`: Defines the Kademlia protocol messages and operations
- `storage`: Provides key-value storage functionality
- `network`: Implements network communication
- `error`: Defines error types

## Usage

### Running a Bootstrap Node

```bash
cargo run --example simple_node -- --port 8000 bootstrap
```

This starts a bootstrap node that other nodes can connect to.

### Joining the Network

```bash
cargo run --example simple_node -- --port 8001 join --bootstrap 127.0.0.1:8000
```

This starts a node that connects to the bootstrap node to join the network.

### Storing a Key-Value Pair

```bash
cargo run --example simple_node -- --port 8002 store --bootstrap 127.0.0.1:8000 --key mykey --value myvalue
```

This stores the key-value pair in the DHT and then exits automatically.

> **Note:** Make sure to use a unique port number each time you run a command, as ports may remain in use for a short time after a process exits.

### Retrieving a Value by Key

```bash
cargo run --example simple_node -- --port 8003 get --bootstrap 127.0.0.1:8000 --key mykey
```

This retrieves the value associated with the given key from the DHT and then exits automatically.

## Implementation Details

### Node ID

Each node in the Kademlia network is identified by a 160-bit NodeId. The distance between two NodeIds is calculated using the XOR metric, which has properties similar to the Euclidean distance:

- d(x, x) = 0
- d(x, y) > 0, if x ≠ y
- d(x, y) = d(y, x)
- d(x, z) ≤ d(x, y) + d(y, z)

### K-Buckets

The routing table consists of k-buckets, where each bucket stores up to k contacts (nodes). The buckets are organized based on the distance from the local node, with bucket i responsible for nodes that share i bits of prefix with the local node.

### Protocol Operations

The implementation supports the four standard Kademlia operations:

- PING: Check if a node is alive
- STORE: Store a key-value pair on a node
- FIND_NODE: Find the k closest nodes to a target NodeId
- FIND_VALUE: Find a value by key, returning either the value or the k closest nodes

### Network Communication

The implementation uses UDP for network communication, with support for message serialization using bincode. The network layer provides asynchronous sending and receiving of messages.

### Storage

The implementation includes a basic in-memory storage system with support for time-to-live (TTL) for stored values.

## Future Improvements

- Persistent storage
- NAT traversal techniques
- Better handling of node failures
- Optimized message serialization
- More extensive testing and benchmarking
- Security enhancements (encryption, authentication)
- Proper port management and resource cleanup
- Improved timeout handling and error recovery

## License

This project is licensed under the MIT License - see the LICENSE file for details.