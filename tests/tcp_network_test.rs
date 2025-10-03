use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kademlia::network::tcp::TcpNetwork;
use kademlia::network::Node;
use kademlia::node::Node as ContactNode;
use kademlia::protocol::{KademliaMessage, MessageId, Network, RequestMessage, ResponseMessage};
use kademlia::Result;

async fn get_available_tcp_port() -> u16 {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  drop(listener);
  port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tcp_network_ping_pong() -> Result<()> {
  let sender_port = get_available_tcp_port().await;
  let receiver_port = get_available_tcp_port().await;

  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), sender_port);
  let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), receiver_port);

  let sender_tcp = TcpNetwork::new(sender_addr).await?;
  let receiver_tcp = TcpNetwork::new(receiver_addr).await?;

  struct TestHandler {
    network: TcpNetwork,
  }

  #[async_trait]
  impl kademlia::protocol::RequestHandler for TestHandler {
    async fn handle_request(&self, request: RequestMessage, from: SocketAddr) {
      if let RequestMessage::Ping { id, sender } = request {
        let response = ResponseMessage::Pong { request_id: id, sender };
        let _ = self.network.send(from, KademliaMessage::Response(response)).await;
      }
    }
  }

  receiver_tcp
    .set_request_handler(Arc::new(TestHandler {
      network: receiver_tcp.clone(),
    }))
    .await?;

  let sender_node = ContactNode::with_addr(sender_addr);
  let message_id: MessageId = 4242;
  let ping = RequestMessage::Ping {
    id: message_id,
    sender: sender_node,
  };

  sender_tcp.send(receiver_addr, KademliaMessage::Request(ping)).await?;

  let response = sender_tcp.wait_response(message_id, Duration::from_secs(5)).await?;
  match response {
    ResponseMessage::Pong { request_id, .. } => {
      assert_eq!(request_id, message_id);
    }
    other => panic!("Unexpected response: {other:?}"),
  }

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tcp_node_store_and_get() -> Result<()> {
  let bootstrap_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), get_available_tcp_port().await);
  let peer_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), get_available_tcp_port().await);

  let bootstrap = Node::with_tcp(bootstrap_addr).await?;
  bootstrap.start().await?;

  let peer = Node::with_tcp(peer_addr).await?;
  peer.start().await?;

  peer.join(bootstrap_addr).await?;

  bootstrap.store(b"tcp_key", b"tcp_value".to_vec()).await?;

  tokio::time::sleep(Duration::from_millis(200)).await;

  let value = peer.get(b"tcp_key").await?;
  assert_eq!(value, b"tcp_value".to_vec());
  Ok(())
}
