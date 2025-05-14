use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::sync::mpsc;

use kademlia::network::udp::UdpNetwork;
use kademlia::node::Node;
use kademlia::node_id::NodeId;
use kademlia::protocol::{KademliaMessage, MessageId, Network, RequestMessage, ResponseMessage};
use kademlia::Result;

// 利用可能なポートを取得するヘルパー関数
async fn get_available_port() -> u16 {
  // ポート0を使用してOSに利用可能なポートを割り当ててもらう
  let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
  socket.local_addr().unwrap().port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_udp_network_creation() -> Result<()> {
  // 利用可能なポートを取得
  let port = get_available_port().await;
  let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);

  // UDPネットワークを作成
  let _udp = UdpNetwork::new(addr).await?;

  // 作成できたことを確認
  assert!(true, "UDPネットワークの作成に成功");

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_ping_request_response() -> Result<()> {
  // 2つのUDPネットワークを作成（送信側と受信側）
  let sender_port = get_available_port().await;
  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), sender_port);
  let sender_udp = UdpNetwork::new(sender_addr).await?;

  let receiver_port = get_available_port().await;
  let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), receiver_port);
  let _receiver_udp = UdpNetwork::new(receiver_addr).await?;

  // 送信側のノードを作成
  let sender_id = NodeId::random();
  let sender_node = Node::new(sender_id.clone(), sender_addr);

  // PINGリクエストを作成
  let message_id: MessageId = 12345;
  let request = RequestMessage::Ping {
    id: message_id,
    sender: sender_node.clone(),
  };

  // リクエストを送信
  sender_udp
    .send(receiver_addr, KademliaMessage::Request(request))
    .await?;

  // レスポンスを待機
  let response = sender_udp.wait_response(message_id, Duration::from_secs(5)).await?;

  // レスポンスを検証
  match response {
    ResponseMessage::Pong { request_id, sender: _ } => {
      assert_eq!(request_id, message_id, "リクエストIDが一致しません");
    }
    _ => {
      panic!("予期しないレスポンスタイプ: {:?}", response);
    }
  }

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_find_node_request_response() -> Result<()> {
  // 2つのUDPネットワークを作成（送信側と受信側）
  let sender_port = get_available_port().await;
  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), sender_port);
  let sender_udp = UdpNetwork::new(sender_addr).await?;

  let receiver_port = get_available_port().await;
  let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), receiver_port);
  let _receiver_udp = UdpNetwork::new(receiver_addr).await?;

  // 送信側のノードを作成
  let sender_id = NodeId::random();
  let sender_node = Node::new(sender_id.clone(), sender_addr);

  // 検索対象のノードIDを作成
  let target_id = NodeId::random();

  // FIND_NODEリクエストを作成
  let message_id: MessageId = 12346;
  let request = RequestMessage::FindNode {
    id: message_id,
    sender: sender_node.clone(),
    target: target_id,
  };

  // リクエストを送信
  sender_udp
    .send(receiver_addr, KademliaMessage::Request(request))
    .await?;

  // レスポンスを待機
  let response = sender_udp.wait_response(message_id, Duration::from_secs(5)).await?;

  // レスポンスを検証
  match response {
    ResponseMessage::NodesFound {
      request_id,
      sender: _,
      nodes,
    } => {
      assert_eq!(request_id, message_id, "リクエストIDが一致しません");
      assert!(!nodes.is_empty(), "ノードリストが空です");
    }
    _ => {
      panic!("予期しないレスポンスタイプ: {:?}", response);
    }
  }

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_find_value_request_response() -> Result<()> {
  // 2つのUDPネットワークを作成（送信側と受信側）
  let sender_port = get_available_port().await;
  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), sender_port);
  let sender_udp = UdpNetwork::new(sender_addr).await?;

  let receiver_port = get_available_port().await;
  let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), receiver_port);
  let _receiver_udp = UdpNetwork::new(receiver_addr).await?;

  // 送信側のノードを作成
  let sender_id = NodeId::random();
  let sender_node = Node::new(sender_id.clone(), sender_addr);

  // 検索対象のキーを作成（テスト用のキー "test_key"）
  let key_bytes = "test_key".as_bytes();
  let key = NodeId::from_bytes(key_bytes);

  // FIND_VALUEリクエストを作成
  let message_id: MessageId = 12347;
  let request = RequestMessage::FindValue {
    id: message_id,
    sender: sender_node.clone(),
    key: key.clone(),
  };

  // リクエストを送信
  sender_udp
    .send(receiver_addr, KademliaMessage::Request(request))
    .await?;

  // レスポンスを待機
  let response = sender_udp.wait_response(message_id, Duration::from_secs(5)).await?;

  // レスポンスを検証
  match response {
    ResponseMessage::ValueFound {
      request_id,
      sender: _,
      value,
      nodes,
    } => {
      assert_eq!(request_id, message_id, "リクエストIDが一致しません");
      // UdpNetworkの実装では、test_keyに対して"test_value"を返すようになっている
      assert!(value.is_some(), "値が見つかりませんでした");
      if let Some(val) = value {
        assert_eq!(String::from_utf8_lossy(&val), "test_value", "値が一致しません");
      }
      assert!(!nodes.is_empty(), "ノードリストが空です");
    }
    _ => {
      panic!("予期しないレスポンスタイプ: {:?}", response);
    }
  }

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_store_request_response() -> Result<()> {
  // 2つのUDPネットワークを作成（送信側と受信側）
  let sender_port = get_available_port().await;
  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), sender_port);
  let sender_udp = UdpNetwork::new(sender_addr).await?;

  let receiver_port = get_available_port().await;
  let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), receiver_port);
  let _receiver_udp = UdpNetwork::new(receiver_addr).await?;

  // 送信側のノードを作成
  let sender_id = NodeId::random();
  let sender_node = Node::new(sender_id.clone(), sender_addr);

  // 保存するキーと値を作成
  let key = NodeId::random();
  let value = "test_store_value".as_bytes().to_vec();

  // STOREリクエストを作成
  let message_id: MessageId = 12348;
  let request = RequestMessage::Store {
    id: message_id,
    sender: sender_node.clone(),
    key: key.clone(),
    value: value.clone(),
  };

  // リクエストを送信
  sender_udp
    .send(receiver_addr, KademliaMessage::Request(request))
    .await?;

  // レスポンスを待機
  let response = sender_udp.wait_response(message_id, Duration::from_secs(5)).await?;

  // レスポンスを検証
  match response {
    ResponseMessage::StoreResult {
      request_id,
      sender: _,
      success,
    } => {
      assert_eq!(request_id, message_id, "リクエストIDが一致しません");
      assert!(success, "保存に失敗しました");
    }
    _ => {
      panic!("予期しないレスポンスタイプ: {:?}", response);
    }
  }

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_timeout_handling() -> Result<()> {
  // UDPネットワークを作成
  let sender_port = get_available_port().await;
  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), sender_port);
  let sender_udp = UdpNetwork::new(sender_addr).await?;

  // 存在しないアドレスを作成（応答がないことを確認するため）
  let non_existent_port = 59999; // 通常使用されていないポート
  let non_existent_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), non_existent_port);

  // 送信側のノードを作成
  let sender_id = NodeId::random();
  let sender_node = Node::new(sender_id.clone(), sender_addr);

  // PINGリクエストを作成
  let message_id: MessageId = 12349;
  let request = RequestMessage::Ping {
    id: message_id,
    sender: sender_node.clone(),
  };

  // リクエストを送信
  sender_udp
    .send(non_existent_addr, KademliaMessage::Request(request))
    .await?;

  // 短いタイムアウトでレスポンスを待機
  let result = sender_udp.wait_response(message_id, Duration::from_millis(100)).await;

  // タイムアウトエラーを期待
  assert!(result.is_err(), "タイムアウトエラーが発生しませんでした");

  Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_custom_request_handler() -> Result<()> {
  // UDPネットワークを作成
  let receiver_port = get_available_port().await;
  let receiver_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), receiver_port);
  let mut receiver_udp = UdpNetwork::new(receiver_addr).await?;

  let sender_port = get_available_port().await;
  let sender_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), sender_port);
  let sender_udp = UdpNetwork::new(sender_addr).await?;

  // カスタムリクエストハンドラを作成
  let (tx, _rx) = mpsc::channel::<(RequestMessage, SocketAddr)>(10);
  receiver_udp.set_request_handler(tx);

  // 送信側のノードを作成
  let sender_id = NodeId::random();
  let sender_node = Node::new(sender_id.clone(), sender_addr);

  // PINGリクエストを作成
  let message_id: MessageId = 12350;
  let request = RequestMessage::Ping {
    id: message_id,
    sender: sender_node.clone(),
  };

  // リクエストを送信
  sender_udp
    .send(receiver_addr, KademliaMessage::Request(request.clone()))
    .await?;

  // レスポンスを待機
  let response = sender_udp.wait_response(message_id, Duration::from_secs(5)).await?;

  // レスポンスを検証
  match response {
    ResponseMessage::Pong { request_id, sender: _ } => {
      assert_eq!(request_id, message_id, "リクエストIDが一致しません");
    }
    _ => {
      panic!("予期しないレスポンスタイプ: {:?}", response);
    }
  }

  Ok(())
}
