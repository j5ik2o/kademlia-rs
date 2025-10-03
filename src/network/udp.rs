use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use crate::protocol::{KademliaMessage, MessageId, Network, RequestHandler, ResponseMessage};
use crate::{Error, Result};

const MAX_DATAGRAM_SIZE: usize = 65507; // Max UDP packet size

/// Rate limiting parameters
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60); // 1 minute window
const MAX_REQUESTS_PER_WINDOW: usize = 100; // Max requests per IP per window

/// Tracks rate limiting information for an IP address
#[derive(Debug, Clone)]
struct RateLimitEntry {
  /// Timestamp of the window start
  window_start: Instant,
  /// Number of requests in the current window
  request_count: usize,
}

/// UDP implementation of the Network trait
pub struct UdpNetwork {
  /// The local socket (held to keep the listener alive for background tasks)
  _socket: Arc<UdpSocket>,
  /// Channel for sending outgoing messages
  sender: mpsc::Sender<(SocketAddr, Vec<u8>)>,
  /// Map of pending response channels keyed by message ID
  pending_responses: Arc<Mutex<HashMap<MessageId, oneshot::Sender<ResponseMessage>>>>,
  /// Handler for incoming request messages
  request_handler: Arc<Mutex<Option<Arc<dyn RequestHandler>>>>,
  /// Rate limiting table keyed by IP address
  #[allow(dead_code)]
  rate_limit_table: Arc<Mutex<HashMap<IpAddr, RateLimitEntry>>>,
}

impl UdpNetwork {
  /// Create a new UDP network interface and start the background tasks
  pub async fn new(bind_addr: SocketAddr) -> Result<Self> {
    let socket = UdpSocket::bind(bind_addr).await?;
    info!(bind_addr = %bind_addr, "UDP socket bound to address");
    let socket = Arc::new(socket);

    let (tx, rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(100);
    let pending_responses = Arc::new(Mutex::new(HashMap::new()));
    let request_handler = Arc::new(Mutex::new(None));
    let rate_limit_table = Arc::new(Mutex::new(HashMap::new()));

    let udp = UdpNetwork {
      _socket: socket.clone(),
      sender: tx,
      pending_responses: pending_responses.clone(),
      request_handler: request_handler.clone(),
      rate_limit_table: rate_limit_table.clone(),
    };

    // Spawn task for handling incoming messages
    let socket_clone = socket.clone();
    let pending_clone = pending_responses.clone();
    let handler_clone = request_handler.clone();
    let rate_limit_clone = rate_limit_table.clone();

    tokio::spawn(async move {
      info!("Starting incoming message handler");
      UdpNetwork::handle_incoming(socket_clone, pending_clone, handler_clone, rate_limit_clone).await;
    });

    // Spawn task for sending outgoing messages
    let socket_clone = socket.clone();
    tokio::spawn(async move {
      info!("Starting outgoing message handler");
      UdpNetwork::handle_outgoing(socket_clone, rx).await;
    });

    Ok(udp)
  }

  /// Check if an IP address is rate limited
  fn check_rate_limit(rate_limit_table: &mut HashMap<IpAddr, RateLimitEntry>, ip: IpAddr) -> bool {
    let now = Instant::now();

    let entry = rate_limit_table.entry(ip).or_insert_with(|| RateLimitEntry {
      window_start: now,
      request_count: 0,
    });

    // Check if the window has expired
    if now.duration_since(entry.window_start) > RATE_LIMIT_WINDOW {
      // Reset window
      entry.window_start = now;
      entry.request_count = 0;
    }

    // Check if limit is exceeded
    if entry.request_count >= MAX_REQUESTS_PER_WINDOW {
      warn!(ip = %ip, count = entry.request_count, "Rate limit exceeded for IP");
      return false;
    }

    // Increment counter
    entry.request_count += 1;
    true
  }

  /// Task for handling incoming messages
  async fn handle_incoming(
    socket: Arc<UdpSocket>,
    pending_responses: Arc<Mutex<HashMap<MessageId, oneshot::Sender<ResponseMessage>>>>,
    request_handler: Arc<Mutex<Option<Arc<dyn RequestHandler>>>>,
    rate_limit_table: Arc<Mutex<HashMap<IpAddr, RateLimitEntry>>>,
  ) {
    let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];

    loop {
      match socket.recv_from(&mut buf).await {
        Ok((size, src)) => {
          if size > 0 {
            // Try to deserialize the message
            match bincode::deserialize::<KademliaMessage>(&buf[..size]) {
              Ok(message) => {
                debug!(source = %src, "Received message");
                match message {
                  KademliaMessage::Response(response) => {
                    debug!(request_id = response.request_id(), "Got response");
                    // Get the response channel for this message ID
                    let request_id = response.request_id();
                    let mut pending = pending_responses.lock().await;

                    if let Some(sender) = pending.remove(&request_id) {
                      // Send the response to the waiting task
                      debug!("Forwarding response to waiting task");
                      let _ = sender.send(response);
                    } else {
                      debug!(request_id = request_id, "No pending request found for ID");
                    }
                  }
                  KademliaMessage::Request(request) => {
                    debug!(request_type = ?request, "Got request");

                    // Rate limit check for incoming requests
                    let ip = src.ip();
                    let is_allowed = {
                      let mut rate_limit = rate_limit_table.lock().await;
                      Self::check_rate_limit(&mut rate_limit, ip)
                    };

                    if !is_allowed {
                      warn!(ip = %ip, "Dropping request due to rate limit");
                      continue;
                    }

                    let handler_opt = { request_handler.lock().await.clone() };

                    if let Some(handler) = handler_opt {
                      let handler = handler.clone();
                      tokio::spawn(async move {
                        handler.handle_request(request, src).await;
                      });
                    } else {
                      warn!("No request handler set, dropping request");
                    }
                  }
                }
              }
              Err(e) => {
                error!(error = %e, "Failed to deserialize message");
              }
            }
          }
        }
        Err(e) => {
          error!(error = %e, "Error receiving from UDP socket");
        }
      }
    }
  }

  /// Task for sending outgoing messages with retry mechanism
  async fn handle_outgoing(socket: Arc<UdpSocket>, mut rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>) {
    while let Some((addr, data)) = rx.recv().await {
      debug!(target_addr = %addr, "Sending message");

      // リトライパラメータを設定
      const MAX_RETRIES: usize = 3;
      const RETRY_DELAY_MS: u64 = 500;

      let mut success = false;

      // リトライループ
      for attempt in 1..=MAX_RETRIES {
        match socket.send_to(&data, addr).await {
          Ok(_) => {
            debug!(target_addr = %addr, attempt = attempt, "Message sent successfully");
            success = true;
            break;
          }
          Err(e) => {
            if attempt < MAX_RETRIES {
              warn!(target_addr = %addr, attempt = attempt, max_retries = MAX_RETRIES, error = %e, "Error sending to address. Retrying...");
              tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
            } else {
              error!(target_addr = %addr, max_retries = MAX_RETRIES, error = %e, "Error sending to address after maximum attempts");
            }
          }
        }
      }

      if !success {
        error!(target_addr = %addr, max_retries = MAX_RETRIES, "Failed to send message after maximum attempts");
      }
    }
  }
}

#[async_trait]
impl Network for UdpNetwork {
  /// Send a message to a specific node address
  async fn send(&self, to: SocketAddr, message: KademliaMessage) -> Result<()> {
    // Serialize the message
    let data = bincode::serialize(&message).map_err(|e| Error::Other(format!("Serialization error: {}", e)))?;

    // Send the message through the channel
    self
      .sender
      .send((to, data))
      .await
      .map_err(|_| Error::Network("Failed to send message".to_string()))?;

    Ok(())
  }

  /// Wait for a response to a specific request
  async fn wait_response(&self, request_id: MessageId, wait_timeout: Duration) -> Result<ResponseMessage> {
    let (tx, rx) = oneshot::channel();

    // Register the response channel
    {
      let mut pending = self.pending_responses.lock().await;
      pending.insert(request_id, tx);
    }

    debug!(request_id = request_id, "Waiting for response");

    // Wait for the response with timeout
    match timeout(wait_timeout, rx).await {
      Ok(result) => match result {
        Ok(response) => {
          debug!(request_id = request_id, "Received response");
          Ok(response)
        }
        Err(_) => {
          warn!(request_id = request_id, "Response channel closed");
          Err(Error::Network("Response channel closed".to_string()))
        }
      },
      Err(_) => {
        // Remove the pending response on timeout
        let mut pending = self.pending_responses.lock().await;
        pending.remove(&request_id);
        warn!(request_id = request_id, "Timeout waiting for response");
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
