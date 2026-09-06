//! OGS realtime WebSocket wire protocol.
//!
//! OGS does not use Socket.IO: the realtime endpoint is a plain WebSocket at
//! `wss://online-go.com/` speaking a JSON-array protocol (`[event, payload]` or
//! `[event, payload, id]` for requests). This module owns the framing and the
//! blocking `tungstenite` adapter; the client state machine lives in
//! `ogs_client`.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
};
use std::time::Duration;

use serde_json::Value;
use tungstenite::Message;
use url::Url;

/// Canonical OGS realtime endpoint. Raw WebSockets are the official transport
/// and live at the `/ws` path (not the site root, which is the SPA shell).
pub const OGS_SOCKET_URL: &str = "wss://online-go.com/ws";
/// OGS realtime endpoint pool:
/// - `wsp.online-go.com`: Google Premium Network gateway (bypasses Cloudflare WebSocket throttling/timeouts on macOS)
/// - `online-go.com`: Cloudflare gateway
/// - `wss.online-go.com`: Direct public internet gateway
pub const OGS_FALLBACK_SOCKET_URLS: &[&str] = &[
    "wss://wsp.online-go.com/ws",
    "wss://online-go.com/ws",
    "wss://wss.online-go.com/ws",
];

/// Per-address TCP connect budget. A blocked/slow route (e.g. Cloudflare being
/// throttled on macOS) must fail fast instead of blocking the worker thread on
/// the OS's default SYN retransmission timeout.
const OGS_CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
/// Budget for the TLS + WebSocket handshake once a TCP connection is open.
const OGS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);
/// Read timeout used by the reader loop to poll for inbound frames while still
/// draining outbound commands between reads.
const OGS_POLL_READ_TIMEOUT: Duration = Duration::from_millis(250);
pub const OGS_DEVICE_LANGUAGE: &str = "zh";
pub const OGS_LANGUAGE_VERSION: &str = "1.0";
pub const OGS_CLIENT_VERSION: &str = "0.1";

/// An inbound message classified as either a server event or a request reply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OgsIncoming {
    Event {
        event: String,
        payload: Value,
    },
    Response {
        id: u64,
        payload: Value,
        error: Option<Value>,
    },
}

/// Encodes a fire-and-forget event: `[event, payload]`.
pub fn encode_event(event: &str, payload: &Value) -> String {
    serde_json::json!([event, payload]).to_string()
}

/// Encodes a request that expects a reply: `[event, payload, id]`.
pub fn encode_request(event: &str, payload: &Value, id: u64) -> String {
    serde_json::json!([event, payload, id]).to_string()
}

/// Decodes a raw inbound message. A leading string is an event; a leading
/// integer is a request reply.
pub fn decode_incoming(text: &str) -> Result<OgsIncoming, String> {
    let data: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid OGS socket JSON: {error}"))?;
    let array = data
        .as_array()
        .filter(|array| !array.is_empty())
        .ok_or_else(|| "OGS socket message was not a non-empty array".to_owned())?;
    if let Some(event) = array[0].as_str() {
        Ok(OgsIncoming::Event {
            event: event.to_owned(),
            payload: array.get(1).cloned().unwrap_or(Value::Null),
        })
    } else if let Some(id) = array[0].as_u64() {
        Ok(OgsIncoming::Response {
            id,
            payload: array.get(1).cloned().unwrap_or(Value::Null),
            error: array.get(2).cloned().filter(|error| !error.is_null()),
        })
    } else {
        Err("OGS socket message had an unsupported first element".to_owned())
    }
}

/// Builds the `authenticate` payload for a JWT and a freshly generated device.
pub fn build_authenticate_payload(jwt: &str, device_id: &str, user_agent: &str) -> Value {
    serde_json::json!({
        "jwt": jwt,
        "device_id": device_id,
        "user_agent": user_agent,
        "language": OGS_DEVICE_LANGUAGE,
        "language_version": OGS_LANGUAGE_VERSION,
        "client_version": OGS_CLIENT_VERSION,
    })
}

/// Wire-level WebSocket boundary, injectable for hermetic tests.
pub trait OgsWebSocketTransport: Send + Sync {
    fn connect(&self, url: &str) -> Result<(), String>;
    fn send_text(&self, message: &str) -> Result<(), String>;
    /// Returns the next received text message, or `Ok(None)` on timeout or a
    /// clean close. A fatal transport error is returned as `Err`.
    fn recv_text(&self, timeout: Duration) -> Result<Option<String>, String>;
    fn close(&self);
}

/// Constructs a fresh one-shot transport for each OGS connection attempt.
/// Keeping this boundary injectable makes session lifecycle tests independent
/// from the network and prevents a reconnect from reusing a stale socket.
pub trait OgsWebSocketTransportFactory: Send + Sync {
    fn create(&self) -> Arc<dyn OgsWebSocketTransport>;
}

#[derive(Default)]
pub struct TungsteniteOgsWebSocketTransportFactory;

impl OgsWebSocketTransportFactory for TungsteniteOgsWebSocketTransportFactory {
    fn create(&self) -> Arc<dyn OgsWebSocketTransport> {
        Arc::new(TungsteniteOgsWebSocketTransport::new())
    }
}

enum SocketCommand {
    Connect {
        url: String,
        ready: Sender<Result<(), String>>,
    },
    Send(String),
    Close,
}

/// A background-thread WebSocket adapter. The worker owns the blocking
/// `tungstenite` socket and drains commands between reads, so the caller can
/// send and receive concurrently without an async runtime. One transport
/// supports one connection; reconnect by constructing a fresh transport.
pub struct TungsteniteOgsWebSocketTransport {
    command_tx: Sender<SocketCommand>,
    event_rx: std::sync::Mutex<Receiver<Result<String, String>>>,
    connected: Arc<AtomicBool>,
}

impl TungsteniteOgsWebSocketTransport {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel::<SocketCommand>();
        let (event_tx, event_rx) = mpsc::channel::<Result<String, String>>();
        let connected = Arc::new(AtomicBool::new(false));
        let connected_flag = Arc::clone(&connected);
        std::thread::Builder::new()
            .name("ogs-websocket".to_owned())
            .spawn(move || {
                socket_worker(command_rx, event_tx, connected_flag);
            })
            .ok();
        Self {
            command_tx,
            event_rx: std::sync::Mutex::new(event_rx),
            connected,
        }
    }
}

impl Default for TungsteniteOgsWebSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl OgsWebSocketTransport for TungsteniteOgsWebSocketTransport {
    fn connect(&self, url: &str) -> Result<(), String> {
        if self.connected.load(Ordering::SeqCst) {
            return Err("OGS socket is already connected".to_owned());
        }
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        self.command_tx
            .send(SocketCommand::Connect {
                url: url.to_owned(),
                ready: ready_tx,
            })
            .map_err(|_| "OGS socket worker has stopped".to_owned())?;
        ready_rx
            .recv_timeout(Duration::from_secs(25))
            .map_err(|_| "OGS socket connection timed out".to_owned())?
    }

    fn send_text(&self, message: &str) -> Result<(), String> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err("OGS socket is not connected".to_owned());
        }
        self.command_tx
            .send(SocketCommand::Send(message.to_owned()))
            .map_err(|_| "OGS socket worker has stopped".to_owned())
    }

    fn recv_text(&self, timeout: Duration) -> Result<Option<String>, String> {
        let receiver = self.event_rx.lock().map_err(|error| error.to_string())?;
        match receiver.recv_timeout(timeout) {
            Ok(Ok(text)) => Ok(Some(text)),
            Ok(Err(reason)) => Err(reason),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn close(&self) {
        self.connected.store(false, Ordering::SeqCst);
        let _ = self.command_tx.send(SocketCommand::Close);
    }
}

type WsStream = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn socket_worker(
    command_rx: Receiver<SocketCommand>,
    event_tx: Sender<Result<String, String>>,
    connected: Arc<AtomicBool>,
) {
    // Block until the caller issues Connect (or the channel closes).
    let (url, ready) = match command_rx.recv() {
        Ok(SocketCommand::Connect { url, ready }) => (url, ready),
        _ => return,
    };
    let mut ws = match connect_stream(&url, &ready) {
        Some(ws) => ws,
        None => return,
    };
    connected.store(true, Ordering::SeqCst);
    let _ = ready.send(Ok(()));

    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                if event_tx.send(Ok(text.to_string())).is_err() {
                    break;
                }
            }
            Ok(Message::Ping(payload)) => {
                let _ = ws.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) | Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => {}
            Ok(Message::Pong(_)) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                let _ = event_tx.send(Err("OGS socket closed".to_owned()));
                break;
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => {
                let _ = event_tx.send(Err(format!("OGS socket read failed: {error}")));
                break;
            }
        }
        if !drain_commands(&mut ws, &command_rx, &event_tx, &connected) {
            break;
        }
    }
    connected.store(false, Ordering::SeqCst);
}

fn build_ws_request(url: &str) -> Result<tungstenite::http::Request<()>, String> {
    use tungstenite::client::IntoClientRequest;
    let mut req = url
        .into_client_request()
        .map_err(|e| format!("invalid websocket url {url}: {e}"))?;
    req.headers_mut().insert(
        "Origin",
        tungstenite::http::HeaderValue::from_static("https://online-go.com"),
    );
    req.headers_mut().insert(
        "User-Agent",
        tungstenite::http::HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Ryusei/0.1",
        ),
    );
    Ok(req)
}

fn connect_stream(url: &str, ready: &Sender<Result<(), String>>) -> Option<WsStream> {
    let endpoints: Vec<&str> = if url == OGS_SOCKET_URL || OGS_FALLBACK_SOCKET_URLS.contains(&url) {
        OGS_FALLBACK_SOCKET_URLS.to_vec()
    } else {
        vec![url]
    };

    let mut last_error = String::new();
    for endpoint in endpoints {
        match connect_endpoint(endpoint) {
            Ok(ws) => return Some(ws),
            Err(error) => last_error = error,
        }
    }

    let _ = ready.send(Err(last_error));
    None
}

/// Connects to a single OGS endpoint with bounded timeouts. The TCP connect,
/// TLS handshake, and WebSocket upgrade each carry their own budget so a
/// blocked route fails fast and lets the next fallback gateway be tried.
fn connect_endpoint(url: &str) -> Result<WsStream, String> {
    let parsed =
        Url::parse(url).map_err(|error| format!("invalid websocket url {url}: {error}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("websocket url has no host: {url}"))?;
    let port = parsed.port_or_known_default().unwrap_or(443);

    let socket_addr = format!("{host}:{port}");
    let addresses = socket_addr
        .to_socket_addrs()
        .map_err(|error| format!("could not resolve OGS host {host}: {error}"))?;

    let mut stream = None;
    let mut last_tcp_error = String::from("no usable address");
    for address in addresses {
        match TcpStream::connect_timeout(&address, OGS_CONNECT_TIMEOUT) {
            Ok(connected) => {
                if connected.set_nodelay(true).is_err() {
                    continue;
                }
                if connected
                    .set_read_timeout(Some(OGS_HANDSHAKE_TIMEOUT))
                    .is_err()
                {
                    continue;
                }
                if connected
                    .set_write_timeout(Some(OGS_HANDSHAKE_TIMEOUT))
                    .is_err()
                {
                    continue;
                }
                stream = Some(connected);
                break;
            }
            Err(error) => last_tcp_error = error.to_string(),
        }
    }
    let stream =
        stream.ok_or_else(|| format!("OGS WebSocket connect failed to {url}: {last_tcp_error}"))?;

    let req = build_ws_request(url)?;
    let (ws, _) = tungstenite::client_tls(req, stream)
        .map_err(|error| format!("OGS WebSocket handshake failed to {url}: {error}"))?;

    // Switch to the poll-friendly read timeout for the reader loop.
    set_socket_read_timeout(&ws, OGS_POLL_READ_TIMEOUT)?;
    Ok(ws)
}

fn drain_commands(
    ws: &mut WsStream,
    command_rx: &Receiver<SocketCommand>,
    event_tx: &Sender<Result<String, String>>,
    connected: &Arc<AtomicBool>,
) -> bool {
    loop {
        match command_rx.try_recv() {
            Ok(SocketCommand::Send(text)) => {
                if let Err(error) = ws.send(Message::text(text)) {
                    let _ = event_tx.send(Err(format!("OGS socket write failed: {error}")));
                    connected.store(false, Ordering::SeqCst);
                    return false;
                }
            }
            Ok(SocketCommand::Close) => {
                connected.store(false, Ordering::SeqCst);
                let _ = ws.close(None);
                return false;
            }
            Ok(SocketCommand::Connect { .. }) => {}
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
}

fn set_socket_read_timeout(ws: &WsStream, timeout: Duration) -> Result<(), String> {
    match ws.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(stream) => stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| error.to_string()),
        tungstenite::stream::MaybeTlsStream::Rustls(stream) => stream
            .get_ref()
            .set_read_timeout(Some(timeout))
            .map_err(|error| error.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_events_and_requests() {
        let event = encode_event(
            "game/move",
            &serde_json::json!({"game_id": 1, "move": "dd"}),
        );
        assert_eq!(
            decode_incoming(&event).unwrap(),
            OgsIncoming::Event {
                event: "game/move".to_owned(),
                payload: serde_json::json!({"game_id": 1, "move": "dd"}),
            }
        );
        let request = encode_request("authenticate", &serde_json::json!({"jwt": "t"}), 3);
        assert!(request.contains("\"authenticate\""));
        assert!(request.trim_end().ends_with("3]"));
        // A server reply uses the integer request id as its first element.
        let reply = "[3, {\"jwt\": \"t\"}]";
        assert_eq!(
            decode_incoming(reply).unwrap(),
            OgsIncoming::Response {
                id: 3,
                payload: serde_json::json!({"jwt": "t"}),
                error: None,
            }
        );
    }

    #[test]
    fn rejects_malformed_messages() {
        assert!(decode_incoming("not json").is_err());
        assert!(decode_incoming("[]").is_err());
        assert!(decode_incoming("[1.5]").is_err());
    }

    #[test]
    fn builds_authenticate_payload() {
        let payload = build_authenticate_payload("jwt", "device", "ua");
        assert_eq!(payload["jwt"], "jwt");
        assert_eq!(payload["device_id"], "device");
        assert_eq!(payload["user_agent"], "ua");
    }

    #[test]
    fn socket_endpoints_use_the_ws_path() {
        assert_eq!(OGS_SOCKET_URL, "wss://online-go.com/ws");
        for url in OGS_FALLBACK_SOCKET_URLS {
            assert!(
                url.ends_with("/ws"),
                "fallback endpoint must target the /ws realtime path: {url}"
            );
        }
    }

    #[test]
    fn ws_request_carries_browser_like_headers() {
        let req = build_ws_request("wss://online-go.com/ws").expect("build request");
        assert_eq!(
            req.headers().get("Origin").and_then(|v| v.to_str().ok()),
            Some("https://online-go.com")
        );
        assert!(
            req.headers()
                .get("User-Agent")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ua| ua.contains("Ryusei/0.1"))
        );
    }
}
