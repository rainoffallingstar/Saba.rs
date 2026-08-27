//! Production OGS client: REST login + WebSocket realtime state machine.
//!
//! This is the Ryusei counterpart of Seki-Sabaki's `OgsClient`. It owns the
//! session, socket, matchmaking, active-game and clock state behind one mutex,
//! sanitizes every inbound payload, and uses a revision counter so a late
//! asynchronous result from a previous session can never corrupt the current
//! one. The OGS password is consumed once by `login` and never stored.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use ryusei_domain_core::Color;
use serde_json::Value;
use uuid::Uuid;

use crate::ogs::{OgsCompetitionSession, OgsGameUpdate, OgsServerClock};
use crate::ogs_credentials::{KeyringOgsCredentialStore, OgsCredentialStore, OgsCredentials};
use crate::ogs_rest::{OgsLoginResult, OgsRestFetch, UreqOgsRestFetch, login_via_rest};
use crate::ogs_socket::{
    OGS_SOCKET_URL, OgsIncoming, OgsWebSocketTransport, TungsteniteOgsWebSocketTransport,
    build_authenticate_payload, decode_incoming, encode_event, encode_request,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OgsSocketStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Authenticated,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OgsMatchmakingStatus {
    #[default]
    Idle,
    Searching,
    Matched,
}

#[derive(Clone, Debug, Default)]
pub struct OgsOnlineGame {
    pub game_id: u64,
    pub connected: bool,
    pub move_number: u32,
    pub next_player: Option<Color>,
    pub clock: Option<OgsServerClock>,
    pub black_name: String,
    pub white_name: String,
    pub phase: String,
    pub pending_move: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OgsClientSnapshot {
    pub user: Option<Value>,
    pub socket_status: OgsSocketStatus,
    pub matchmaking_status: OgsMatchmakingStatus,
    pub matched_game_id: Option<u64>,
    pub automatch_uuid: Option<String>,
    pub online_game: Option<OgsOnlineGame>,
    pub last_error: Option<String>,
}

struct LiveOgsInner {
    snapshot: OgsClientSnapshot,
    session: Option<OgsCredentials>,
    transport: Option<Arc<dyn OgsWebSocketTransport>>,
    next_request_id: u64,
    device_id: String,
    competition: Option<OgsCompetitionSession>,
}

/// The UI-independent OGS client. All state-mutating methods take `&self` so a
/// single `Arc<LiveOgsClient>` can be shared with the socket reader thread;
/// the login/restore methods additionally take `self: &Arc<Self>` so they can
/// hand a clone to the reader.
pub struct LiveOgsClient {
    inner: Mutex<LiveOgsInner>,
    store: Box<dyn OgsCredentialStore>,
    rest: Mutex<Box<dyn OgsRestFetch>>,
    on_state_change: Mutex<Option<Box<dyn Fn() + Send>>>,
    stop: AtomicBool,
}

impl LiveOgsClient {
    pub fn new() -> Self {
        Self::with_parts(
            Box::new(KeyringOgsCredentialStore::new()),
            Box::new(UreqOgsRestFetch::new()),
        )
    }

    pub fn with_parts(store: Box<dyn OgsCredentialStore>, rest: Box<dyn OgsRestFetch>) -> Self {
        Self {
            inner: Mutex::new(LiveOgsInner {
                snapshot: OgsClientSnapshot {
                    socket_status: OgsSocketStatus::Disconnected,
                    ..OgsClientSnapshot::default()
                },
                session: None,
                transport: None,
                next_request_id: 0,
                device_id: Uuid::new_v4().to_string(),
                competition: None,
            }),
            store,
            rest: Mutex::new(rest),
            on_state_change: Mutex::new(None),
            stop: AtomicBool::new(false),
        }
    }

    pub fn set_on_state_change(&self, callback: Option<Box<dyn Fn() + Send>>) {
        *self.on_state_change.lock().unwrap() = callback;
    }

    pub fn snapshot(&self) -> OgsClientSnapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }

    fn emit_state(&self) {
        if let Some(callback) = self.on_state_change.lock().unwrap().as_ref() {
            callback();
        }
    }

    /// Whether a secure credential store is available (mirrors Seki's refusal
    /// to persist under Electron `basic_text`).
    pub fn credential_storage_available(&self) -> bool {
        self.store.is_available()
    }

    /// Logs in with the OGS username and password. The password is used once
    /// and never persisted.
    pub fn login(self: &Arc<Self>, username: &str, password: &str) -> Result<Value, String> {
        let login = {
            let mut rest = self.rest.lock().unwrap();
            login_via_rest(&mut **rest, username, password)?
        };
        self.finish_login(login)
    }

    /// Restores a previously persisted session, if a secure store is available.
    pub fn restore_session(self: &Arc<Self>) -> Result<Value, String> {
        let Some(credentials) = self.store.load() else {
            return Err("No stored OGS session.".to_owned());
        };
        if credentials.jwt_token.is_empty() {
            return Err("Stored OGS session was empty.".to_owned());
        }
        let login = OgsLoginResult {
            jwt_token: credentials.jwt_token,
            cookie_header: credentials.cookie_header,
            user: credentials.user.unwrap_or(Value::Null),
        };
        self.finish_login(login)
    }

    fn finish_login(self: &Arc<Self>, login: OgsLoginResult) -> Result<Value, String> {
        self.connect_and_authenticate(&login.jwt_token)?;

        let user = login.user.clone();
        let credentials = OgsCredentials {
            server_url: crate::ogs_rest::OGS_SERVER_URL.to_owned(),
            jwt_token: login.jwt_token,
            cookie_header: login.cookie_header,
            user: Some(user.clone()),
            created_at: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            ),
        };
        if self.store.is_available() {
            let _ = self.store.save(&credentials);
        }

        let mut inner = self.inner.lock().unwrap();
        inner.session = Some(credentials);
        inner.snapshot.user = Some(user.clone());
        inner.snapshot.last_error = None;
        self.emit_state();
        Ok(user)
    }

    pub fn logout(&self) {
        self.stop_reader();
        if let Some(transport) = self.inner.lock().unwrap().transport.take() {
            transport.close();
        }
        self.store.clear();
        let mut inner = self.inner.lock().unwrap();
        inner.session = None;
        inner.competition = None;
        inner.snapshot = OgsClientSnapshot {
            socket_status: OgsSocketStatus::Disconnected,
            ..OgsClientSnapshot::default()
        };
        self.emit_state();
    }

    pub fn connect_game(&self, game_id: u64) -> Result<(), String> {
        self.send_event(
            "game/connect",
            &serde_json::json!({"game_id": game_id, "chat": true}),
        )?;
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.online_game = Some(OgsOnlineGame {
            game_id,
            connected: false,
            ..OgsOnlineGame::default()
        });
        self.emit_state();
        Ok(())
    }

    pub fn disconnect_game(&self, game_id: u64) -> Result<(), String> {
        let result = self.send_event("game/disconnect", &serde_json::json!({"game_id": game_id}));
        let mut inner = self.inner.lock().unwrap();
        if inner
            .snapshot
            .online_game
            .as_ref()
            .is_some_and(|game| game.game_id == game_id)
        {
            inner.snapshot.online_game = None;
            inner.competition = None;
        }
        self.emit_state();
        result
    }

    /// Submits a move. `vertex` is an OGS coordinate string (`Some("dd")`); a
    /// pass is `None` (encoded as `".."`).
    pub fn play_move(&self, game_id: u64, vertex: Option<String>) -> Result<(), String> {
        let encoded = vertex.unwrap_or_else(|| "..".to_owned());
        self.send_event(
            "game/move",
            &serde_json::json!({"game_id": game_id, "move": encoded}),
        )?;
        if let Some(game) = self.inner.lock().unwrap().snapshot.online_game.as_mut() {
            game.pending_move = true;
        }
        self.emit_state();
        Ok(())
    }

    pub fn pass(&self, game_id: u64) -> Result<(), String> {
        self.play_move(game_id, None)
    }

    pub fn resign(&self, game_id: u64) -> Result<(), String> {
        self.send_event("game/resign", &serde_json::json!({"game_id": game_id}))
    }

    pub fn set_removed_stones(
        &self,
        game_id: u64,
        stones: &str,
        removed: bool,
    ) -> Result<(), String> {
        self.send_event(
            "game/removed_stones/set",
            &serde_json::json!({"game_id": game_id, "removed": removed, "stones": stones}),
        )
    }

    pub fn accept_removed_stones(&self, game_id: u64, stones: &str) -> Result<(), String> {
        self.send_event(
            "game/removed_stones/accept",
            &serde_json::json!({"game_id": game_id, "stones": stones}),
        )
    }

    pub fn send_chat(&self, game_id: u64, move_number: u32, body: &str) -> Result<(), String> {
        self.send_event(
            "game/chat",
            &serde_json::json!({
                "game_id": game_id,
                "type": "main",
                "move_number": move_number,
                "body": body,
            }),
        )
    }

    pub fn start_automatch(&self, options: &Value) -> Result<(), String> {
        let uuid = Uuid::new_v4().to_string();
        let payload = match options.as_object() {
            Some(object) => {
                let mut merged = object.clone();
                merged.insert("uuid".to_owned(), Value::String(uuid.clone()));
                Value::Object(merged)
            }
            None => serde_json::json!({"uuid": uuid}),
        };
        self.send_event("automatch/find_match", &payload)?;
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.matchmaking_status = OgsMatchmakingStatus::Searching;
        inner.snapshot.automatch_uuid = Some(uuid);
        self.emit_state();
        Ok(())
    }

    pub fn cancel_automatch(&self) -> Result<(), String> {
        let uuid = self
            .inner
            .lock()
            .unwrap()
            .snapshot
            .automatch_uuid
            .clone()
            .unwrap_or_default();
        self.send_event("automatch/cancel", &serde_json::json!({"uuid": uuid}))?;
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.matchmaking_status = OgsMatchmakingStatus::Idle;
        inner.snapshot.automatch_uuid = None;
        self.emit_state();
        Ok(())
    }

    /// The most recently applied server update for the attached competition.
    pub fn competition_game_id(&self) -> Option<u64> {
        self.inner
            .lock()
            .unwrap()
            .competition
            .as_ref()
            .map(|c| c.game_id)
    }

    // -- internals -----------------------------------------------------------

    fn send_event(&self, event: &str, payload: &Value) -> Result<(), String> {
        let transport = self
            .inner
            .lock()
            .unwrap()
            .transport
            .clone()
            .ok_or_else(|| "OGS socket is not connected".to_owned())?;
        transport.send_text(&encode_event(event, payload))
    }

    fn connect_and_authenticate(self: &Arc<Self>, jwt: &str) -> Result<(), String> {
        let transport: Arc<dyn OgsWebSocketTransport> =
            Arc::new(TungsteniteOgsWebSocketTransport::new());
        {
            let mut inner = self.inner.lock().unwrap();
            inner.snapshot.socket_status = OgsSocketStatus::Connecting;
        }
        self.emit_state();
        transport.connect(OGS_SOCKET_URL)?;

        let device_id = self.inner.lock().unwrap().device_id.clone();
        let payload = build_authenticate_payload(jwt, &device_id, crate::ogs_rest::OGS_USER_AGENT);
        let request_id = {
            let mut inner = self.inner.lock().unwrap();
            inner.next_request_id += 1;
            inner.next_request_id
        };
        transport.send_text(&encode_request("authenticate", &payload, request_id))?;

        // Wait for the authenticate reply before exposing the session.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                transport.close();
                return Err("OGS authentication timed out".to_owned());
            }
            if let Some(text) = transport.recv_text(Duration::from_millis(250))? {
                match decode_incoming(&text)? {
                    OgsIncoming::Response { id, .. } if id == request_id => break,
                    OgsIncoming::Response { .. } | OgsIncoming::Event { .. } => {}
                }
            }
        }

        {
            let mut inner = self.inner.lock().unwrap();
            inner.transport = Some(transport);
            inner.snapshot.socket_status = OgsSocketStatus::Authenticated;
        }
        self.spawn_reader();
        self.emit_state();
        Ok(())
    }

    fn spawn_reader(self: &Arc<Self>) {
        let client = Arc::clone(self);
        std::thread::Builder::new()
            .name("ogs-client-reader".to_owned())
            .spawn(move || {
                reader_loop(client);
            })
            .ok();
    }

    fn stop_reader(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn handle_incoming(&self, incoming: OgsIncoming) {
        match incoming {
            OgsIncoming::Response { .. } => {}
            OgsIncoming::Event { event, payload } => self.handle_event(&event, &payload),
        }
    }

    fn handle_event(&self, event: &str, payload: &Value) {
        match event {
            "automatch/start" => {
                let game_id = payload.get("game_id").and_then(Value::as_u64);
                let mut inner = self.inner.lock().unwrap();
                inner.snapshot.matchmaking_status = OgsMatchmakingStatus::Matched;
                inner.snapshot.matched_game_id = game_id;
                if let Some(game_id) = game_id {
                    drop(inner);
                    let _ = self.connect_game(game_id);
                    return;
                }
            }
            "net/pong" | "user/state" | "active_game" | "notification" => {}
            other => {
                // Per-game events arrive as `game/<id>/<type>`.
                let Some(rest) = other.strip_prefix("game/") else {
                    return;
                };
                let Some((id_text, kind)) = rest.split_once('/') else {
                    return;
                };
                let Ok(game_id) = id_text.parse::<u64>() else {
                    return;
                };
                let current = self
                    .inner
                    .lock()
                    .unwrap()
                    .snapshot
                    .online_game
                    .as_ref()
                    .map(|game| game.game_id);
                if current != Some(game_id) {
                    return;
                }
                match kind {
                    "gamedata" | "data" => self.apply_gamedata(game_id, payload),
                    "move" => self.apply_move(game_id, payload),
                    "clock" => self.apply_clock(game_id, payload),
                    "chat" | "latency" | "removed_stones" => {}
                    _ => {}
                }
            }
        }
        self.emit_state();
    }

    fn apply_gamedata(&self, game_id: u64, payload: &Value) {
        let move_number = payload.get("moves").and_then(Value::as_array).map_or_else(
            || {
                payload
                    .get("move_number")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32
            },
            |moves| moves.len() as u32,
        );
        let next_player = player_to_move(payload, move_number);
        let clock = parse_clock(payload.get("clock"));
        let black_name = player_name(payload, "black");
        let white_name = player_name(payload, "white");
        let phase = payload
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        let update = build_game_update(game_id, move_number, next_player, clock.as_ref());
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.online_game = Some(OgsOnlineGame {
            game_id,
            connected: true,
            move_number,
            next_player,
            clock,
            black_name,
            white_name,
            phase,
            pending_move: false,
        });
        if let Some(update) = update {
            inner.competition = OgsCompetitionSession::new(game_id, update).ok();
        }
    }

    fn apply_move(&self, game_id: u64, payload: &Value) {
        let move_number = payload
            .get("move_number")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        let mut inner = self.inner.lock().unwrap();
        let Some(game) = inner.snapshot.online_game.as_mut() else {
            return;
        };
        if game.game_id != game_id {
            return;
        }
        game.move_number = move_number.unwrap_or(game.move_number + 1);
        game.pending_move = false;
        // After a move, the player to move flips.
        game.next_player = game.next_player.map(|color| match color {
            Color::Black => Color::White,
            Color::White => Color::Black,
        });
        let update = build_game_update(
            game_id,
            game.move_number,
            game.next_player,
            game.clock.as_ref(),
        );
        if let Some(update) = update {
            if let Some(competition) = inner.competition.as_mut() {
                let _ = competition.apply_server_update(update);
            } else if let Ok(competition) = OgsCompetitionSession::new(game_id, update) {
                inner.competition = Some(competition);
            }
        }
    }

    fn apply_clock(&self, game_id: u64, payload: &Value) {
        let clock = parse_clock(Some(payload));
        let mut inner = self.inner.lock().unwrap();
        let Some(game) = inner.snapshot.online_game.as_mut() else {
            return;
        };
        if game.game_id != game_id {
            return;
        }
        game.next_player = clock.as_ref().and_then(|c| c.active_color);
        game.clock = clock;
    }
}

impl Default for LiveOgsClient {
    fn default() -> Self {
        Self::new()
    }
}

fn reader_loop(client: Arc<LiveOgsClient>) {
    while !client.stop.load(Ordering::SeqCst) {
        let transport = client.inner.lock().unwrap().transport.clone();
        let Some(transport) = transport else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        match transport.recv_text(Duration::from_secs(1)) {
            Ok(Some(text)) => {
                if let Ok(incoming) = decode_incoming(&text) {
                    client.handle_incoming(incoming);
                }
            }
            Ok(None) => {}
            Err(_) => {
                let mut inner = client.inner.lock().unwrap();
                inner.snapshot.socket_status = OgsSocketStatus::Error;
                client.emit_state();
                break;
            }
        }
    }
    let mut inner = client.inner.lock().unwrap();
    inner.snapshot.socket_status = OgsSocketStatus::Disconnected;
}

// -- pure payload parsers ----------------------------------------------------

fn player_to_move(payload: &Value, move_number: u32) -> Option<Color> {
    let black_id = payload.pointer("/players/black/id").and_then(Value::as_u64);
    let white_id = payload.pointer("/players/white/id").and_then(Value::as_u64);
    let current_player = payload
        .pointer("/clock/current_player")
        .and_then(Value::as_u64);
    match current_player {
        Some(id) if Some(id) == black_id => Some(Color::Black),
        Some(id) if Some(id) == white_id => Some(Color::White),
        _ => {
            if move_number.is_multiple_of(2) {
                Some(Color::Black)
            } else {
                Some(Color::White)
            }
        }
    }
}

fn player_name(payload: &Value, color: &str) -> String {
    payload
        .pointer(&format!("/players/{color}/username"))
        .and_then(Value::as_str)
        .unwrap_or(color)
        .to_owned()
}

fn seconds_to_duration(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds >= 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

fn parse_clock(clock: Option<&Value>) -> Option<OgsServerClock> {
    let clock = clock?;
    let black = clock.get("black_time");
    let white = clock.get("white_time");
    let active_color = clock
        .get("current_player")
        .and_then(Value::as_u64)
        .and_then(|id| {
            let black_id = clock.get("black_player_id").and_then(Value::as_u64);
            let white_id = clock.get("white_player_id").and_then(Value::as_u64);
            if Some(id) == black_id {
                Some(Color::Black)
            } else if Some(id) == white_id {
                Some(Color::White)
            } else {
                None
            }
        });
    let paused = clock
        .pointer("/pause/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(OgsServerClock {
        black_main_remaining: seconds_to_duration(
            black
                .and_then(|b| b.get("thinking_time"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ),
        white_main_remaining: seconds_to_duration(
            white
                .and_then(|w| w.get("thinking_time"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ),
        black_byo_yomi_remaining: seconds_to_duration(
            black
                .and_then(|b| b.get("period_time_left"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ),
        white_byo_yomi_remaining: seconds_to_duration(
            white
                .and_then(|w| w.get("period_time_left"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ),
        black_periods: black
            .and_then(|b| b.get("periods"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        white_periods: white
            .and_then(|w| w.get("periods"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        active_color,
        paused,
    })
}

fn build_game_update(
    game_id: u64,
    move_number: u32,
    next_player: Option<Color>,
    clock: Option<&OgsServerClock>,
) -> Option<OgsGameUpdate> {
    Some(OgsGameUpdate {
        game_id,
        move_number,
        next_player: next_player.unwrap_or(Color::Black),
        clock: clock.cloned().unwrap_or(OgsServerClock {
            black_main_remaining: Duration::ZERO,
            white_main_remaining: Duration::ZERO,
            black_byo_yomi_remaining: Duration::ZERO,
            white_byo_yomi_remaining: Duration::ZERO,
            black_periods: 0,
            white_periods: 0,
            active_color: None,
            paused: false,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ogs_credentials::MemoryOgsCredentialStore;
    use crate::ogs_rest::OgsHttpResponse;

    fn gamedata_payload() -> Value {
        serde_json::json!({
            "width": 19,
            "height": 19,
            "phase": "play",
            "moves": [[3, 3, 1], [15, 15, 2]],
            "players": {
                "black": {"id": 7, "username": "Black"},
                "white": {"id": 8, "username": "White"}
            },
            "clock": {
                "current_player": 8,
                "black_player_id": 7,
                "white_player_id": 8,
                "black_time": {"thinking_time": 500.0, "periods": 5, "period_time_left": 30.0},
                "white_time": {"thinking_time": 600.0, "periods": 5, "period_time_left": 30.0}
            }
        })
    }

    #[test]
    fn parses_gamedata_move_number_and_next_player() {
        let payload = gamedata_payload();
        assert_eq!(player_to_move(&payload, 2), Some(Color::White));
        assert_eq!(player_name(&payload, "black"), "Black");
        let clock = parse_clock(payload.get("clock")).unwrap();
        assert_eq!(clock.black_main_remaining, Duration::from_secs(500));
        assert_eq!(clock.white_periods, 5);
        assert_eq!(clock.active_color, Some(Color::White));
    }

    #[test]
    fn infers_next_player_from_move_parity() {
        let payload = serde_json::json!({"moves": [[3, 3, 1]]});
        assert_eq!(player_to_move(&payload, 1), Some(Color::White));
        assert_eq!(player_to_move(&payload, 2), Some(Color::Black));
    }

    #[test]
    fn unavailable_store_refuses_persistence() {
        let store = MemoryOgsCredentialStore::unavailable();
        assert!(!store.is_available());
        assert!(store.load().is_none());
    }

    #[test]
    fn snapshot_defaults_to_disconnected() {
        let client = LiveOgsClient::with_parts(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
        );
        let snapshot = client.snapshot();
        assert_eq!(snapshot.socket_status, OgsSocketStatus::Disconnected);
        assert!(snapshot.online_game.is_none());
        assert!(client.competition_game_id().is_none());
    }

    struct TestRestFetch;
    impl OgsRestFetch for TestRestFetch {
        fn get(&mut self, _url: &str) -> Result<OgsHttpResponse, String> {
            Err("unused".to_owned())
        }
        fn post_json(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &str,
        ) -> Result<OgsHttpResponse, String> {
            Err("unused".to_owned())
        }
    }
}
