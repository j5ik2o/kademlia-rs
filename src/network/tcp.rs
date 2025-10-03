use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use crate::protocol::{KademliaMessage, MessageId, Network, RequestHandler, ResponseMessage};
use crate::{Error, Result};

/// フレーム長の最大値（16MiBを上限とする）
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// TCPベースのNetwork実装
#[derive(Clone)]
pub struct TcpNetwork {
  /// リスナーを保持しバックグラウンドタスクの存続を確保
  _listener: Arc<TcpListener>,
  /// 送信用チャンネル
  sender: mpsc::Sender<(SocketAddr, Vec<u8>)>,
  /// レスポンス待機チャネル
  pending_responses: Arc<Mutex<HashMap<MessageId, oneshot::Sender<ResponseMessage>>>>,
  /// リクエストハンドラ
  request_handler: Arc<Mutex<Option<Arc<dyn RequestHandler>>>>,
}

impl TcpNetwork {
  /// 指定アドレスでTCPリスナーを立ち上げ、入出力タスクを起動
  pub async fn new(bind_addr: SocketAddr) -> Result<Self> {
    let listener = TcpListener::bind(bind_addr).await?;
    info!(bind_addr = %bind_addr, "TCP listener started");
    let listener = Arc::new(listener);

    let (tx, rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(100);
    let pending_responses = Arc::new(Mutex::new(HashMap::new()));
    let request_handler = Arc::new(Mutex::new(None));

    let network = TcpNetwork {
      _listener: listener.clone(),
      sender: tx,
      pending_responses: pending_responses.clone(),
      request_handler: request_handler.clone(),
    };

    // Acceptループ
    let listener_clone = listener.clone();
    tokio::spawn(async move {
      info!("Starting TCP accept loop");
      loop {
        match listener_clone.accept().await {
          Ok((stream, peer)) => {
            debug!(peer = %peer, "Accepted TCP connection");
            let pending = pending_responses.clone();
            let handler = request_handler.clone();
            let peer_addr = peer;
            tokio::spawn(async move {
              if let Err(err) = TcpNetwork::handle_connection(stream, pending, handler).await {
                error!(peer = %peer_addr, ?err, "TCP connection handler exited with error");
              }
            });
          }
          Err(err) => {
            error!(?err, "Failed to accept TCP connection");
          }
        }
      }
    });

    // 送信処理
    tokio::spawn(async move {
      info!("Starting TCP outgoing handler");
      TcpNetwork::handle_outgoing(rx).await;
    });

    Ok(network)
  }

  async fn handle_connection(
    mut stream: TcpStream,
    pending_responses: Arc<Mutex<HashMap<MessageId, oneshot::Sender<ResponseMessage>>>>,
    request_handler: Arc<Mutex<Option<Arc<dyn RequestHandler>>>>,
  ) -> Result<()> {
    loop {
      let frame = match TcpNetwork::read_frame(&mut stream).await {
        Ok(Some(data)) => data,
        Ok(None) => {
          debug!("TCP peer closed the connection");
          break;
        }
        Err(err) => {
          return Err(err);
        }
      };

      match bincode::deserialize::<KademliaMessage>(&frame) {
        Ok(KademliaMessage::Response(response)) => {
          let request_id = response.request_id();
          let mut pending = pending_responses.lock().await;
          if let Some(sender) = pending.remove(&request_id) {
            let _ = sender.send(response);
          } else {
            debug!(request_id = request_id, "No pending request for TCP response");
          }
        }
        Ok(KademliaMessage::Request(request)) => {
          let handler_opt = { request_handler.lock().await.clone() };
          if let Some(handler) = handler_opt {
            let reply_addr = request.sender().addr;
            let handler = handler.clone();
            tokio::spawn(async move {
              handler.handle_request(request, reply_addr).await;
            });
          } else {
            warn!("No TCP request handler registered; dropping request");
          }
        }
        Err(err) => {
          error!(?err, "Failed to deserialize TCP frame");
        }
      }
    }

    Ok(())
  }

  async fn read_frame(stream: &mut TcpStream) -> Result<Option<Vec<u8>>> {
    match stream.read_u32().await {
      Ok(len) => {
        let len = len as usize;
        if len == 0 || len > MAX_FRAME_SIZE {
          return Err(Error::Network(format!("Invalid TCP frame size: {len}")));
        }
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        Ok(Some(buf))
      }
      Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
      Err(err) => Err(err.into()),
    }
  }

  async fn write_frame(mut stream: TcpStream, data: Vec<u8>) -> Result<()> {
    let len = data.len();
    if len > MAX_FRAME_SIZE {
      return Err(Error::Network("Frame too large".to_string()));
    }
    stream.write_u32(len as u32).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
  }

  async fn handle_outgoing(mut rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>) {
    while let Some((addr, data)) = rx.recv().await {
      match TcpStream::connect(addr).await {
        Ok(stream) => {
          if let Err(err) = TcpNetwork::write_frame(stream, data).await {
            error!(target_addr = %addr, ?err, "Failed to send TCP frame");
          }
        }
        Err(err) => {
          error!(target_addr = %addr, ?err, "Failed to connect to TCP peer");
        }
      }
    }
  }
}

#[async_trait]
impl Network for TcpNetwork {
  async fn send(&self, to: SocketAddr, message: KademliaMessage) -> Result<()> {
    let data = bincode::serialize(&message).map_err(|e| Error::Other(format!("Serialization error: {}", e)))?;
    self
      .sender
      .send((to, data))
      .await
      .map_err(|_| Error::Network("Failed to enqueue TCP message".to_string()))
  }

  async fn wait_response(&self, request_id: MessageId, wait_timeout: Duration) -> Result<ResponseMessage> {
    let (tx, rx) = oneshot::channel();
    {
      let mut pending = self.pending_responses.lock().await;
      pending.insert(request_id, tx);
    }

    match timeout(wait_timeout, rx).await {
      Ok(Ok(response)) => Ok(response),
      Ok(Err(_)) => Err(Error::Network("Response channel closed".to_string())),
      Err(_) => {
        let mut pending = self.pending_responses.lock().await;
        pending.remove(&request_id);
        Err(Error::Timeout)
      }
    }
  }

  async fn set_request_handler(&self, handler: Arc<dyn RequestHandler>) -> Result<()> {
    let mut guard = self.request_handler.lock().await;
    *guard = Some(handler);
    Ok(())
  }
}
