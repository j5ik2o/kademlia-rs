use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::protocol::{KademliaMessage, MessageId, Network, RequestHandler, RequestMessage, ResponseMessage};
use crate::{Error, Result};

/// Shared registry mapping socket addresses to in-memory network transports.
fn registry() -> &'static Mutex<HashMap<SocketAddr, Weak<InMemoryNetworkInner>>> {
  static REGISTRY: OnceLock<Mutex<HashMap<SocketAddr, Weak<InMemoryNetworkInner>>>> = OnceLock::new();
  REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

struct InMemoryNetworkInner {
  addr: SocketAddr,
  pending_responses: Mutex<HashMap<MessageId, oneshot::Sender<ResponseMessage>>>,
  request_handler: Mutex<Option<Arc<dyn RequestHandler>>>,
}

impl InMemoryNetworkInner {
  async fn deliver_response(&self, response: ResponseMessage) {
    let mut pending = self.pending_responses.lock().await;
    if let Some(sender) = pending.remove(&response.request_id()) {
      let _ = sender.send(response);
    } else {
      debug!(request_id = response.request_id(), "No pending request for response");
    }
  }

  async fn forward_request(&self, request: RequestMessage, from: SocketAddr) {
    let handler_opt = { self.request_handler.lock().await.clone() };

    if let Some(handler) = handler_opt {
      tokio::spawn(async move {
        handler.handle_request(request, from).await;
      });
    } else {
      warn!(addr = %self.addr, "No request handler registered for in-memory transport");
    }
  }
}

impl Drop for InMemoryNetworkInner {
  fn drop(&mut self) {
    if let Ok(mut guard) = registry().try_lock() {
      guard.remove(&self.addr);
    }
  }
}

/// In-memory Network 実装。テストやシミュレーション用に使用する。
#[derive(Clone)]
pub struct InMemoryNetwork {
  inner: Arc<InMemoryNetworkInner>,
}

impl InMemoryNetwork {
  /// 指定アドレスで新しいインメモリトランスポートを作成する。重複アドレスはエラー。
  pub async fn new(addr: SocketAddr) -> Result<Self> {
    let inner = Arc::new(InMemoryNetworkInner {
      addr,
      pending_responses: Mutex::new(HashMap::new()),
      request_handler: Mutex::new(None),
    });

    let mut guard = registry().lock().await;
    if let Some(existing) = guard.get(&addr) {
      if existing.upgrade().is_some() {
        return Err(Error::Network(format!("Address {addr} already registered")));
      }
    }

    guard.insert(addr, Arc::downgrade(&inner));

    Ok(Self { inner })
  }

  fn addr(&self) -> SocketAddr {
    self.inner.addr
  }

  async fn lookup(addr: SocketAddr) -> Option<Arc<InMemoryNetworkInner>> {
    let mut guard = registry().lock().await;
    if let Some(existing) = guard.get(&addr) {
      if let Some(inner) = existing.upgrade() {
        return Some(inner);
      }

      guard.remove(&addr);
    }

    None
  }
}

#[async_trait]
impl Network for InMemoryNetwork {
  async fn send(&self, to: SocketAddr, message: KademliaMessage) -> Result<()> {
    let remote = Self::lookup(to)
      .await
      .ok_or_else(|| Error::Network(format!("No in-memory transport registered for {to}")))?;

    match message {
      KademliaMessage::Response(response) => {
        remote.deliver_response(response).await;
      }
      KademliaMessage::Request(request) => {
        let remote_addr = self.addr();
        remote.forward_request(request, remote_addr).await;
      }
    }

    Ok(())
  }

  async fn wait_response(&self, request_id: MessageId, wait_timeout: Duration) -> Result<ResponseMessage> {
    let (tx, rx) = oneshot::channel();

    {
      let mut pending = self.inner.pending_responses.lock().await;
      pending.insert(request_id, tx);
    }

    match timeout(wait_timeout, rx).await {
      Ok(Ok(response)) => Ok(response),
      Ok(Err(_)) => Err(Error::Network("Response channel closed".to_string())),
      Err(_) => {
        let mut pending = self.inner.pending_responses.lock().await;
        pending.remove(&request_id);
        Err(Error::Timeout)
      }
    }
  }

  async fn set_request_handler(&self, handler: Arc<dyn RequestHandler>) -> Result<()> {
    let mut guard = self.inner.request_handler.lock().await;
    *guard = Some(handler);
    Ok(())
  }
}
