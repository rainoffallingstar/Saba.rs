//! Production OGS client: REST login + WebSocket realtime state machine.
//!
//! This is the Ryusei counterpart of Seki-Sabaki's `OgsClient`. It owns the
//! session, socket, matchmaking, active-game and clock state behind one mutex,
//! sanitizes every inbound payload, and uses a revision counter so a late
//! asynchronous result from a previous session can never corrupt the current
//! one. The OGS password is consumed once by `login` and never stored.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ryusei_domain_core::{Color, TimeControl};
use serde_json::Value;
use uuid::Uuid;

use crate::ogs::{OgsCompetitionSession, OgsGameUpdate, OgsServerClock};
use crate::ogs_credentials::{KeyringOgsCredentialStore, OgsCredentialStore, OgsCredentials};
use crate::ogs_rest::{OgsLoginResult, OgsRestFetch, UreqOgsRestFetch, login_via_rest};
use crate::ogs_socket::{
    OGS_SOCKET_URL, OgsIncoming, OgsWebSocketTransport, OgsWebSocketTransportFactory,
    TungsteniteOgsWebSocketTransportFactory, build_authenticate_payload, decode_incoming,
    encode_event, encode_request,
};

/// Maximum retained chat lines per game; older lines are dropped on overflow.
const MAX_CHAT_LINES: usize = 200;
/// Maximum characters retained per chat message body.
const MAX_CHAT_BODY_CHARS: usize = 4096;
/// Maximum characters retained per chat username.
const MAX_CHAT_USERNAME_CHARS: usize = 64;
/// Reconnect delays are capped so a transient OGS outage cannot strand a live
/// game indefinitely. The test build uses a short delay to keep lifecycle tests
/// fast while exercising the same retry state machine.
const OGS_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

fn ogs_heartbeat_interval() -> Duration {
    if cfg!(test) {
        Duration::from_millis(10)
    } else {
        Duration::from_secs(20)
    }
}

fn ogs_reconnect_delay(attempt: u32) -> Duration {
    if cfg!(test) {
        Duration::from_millis(10u64.saturating_mul(1u64 << attempt.min(6)))
    } else {
        Duration::from_secs(1u64 << attempt.min(5)).min(OGS_RECONNECT_MAX_DELAY)
    }
}

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
    /// Server player ids, used to gate submissions on participant identity.
    pub black_id: Option<u64>,
    pub white_id: Option<u64>,
    pub phase: String,
    pub pending_move: bool,
    pub width: u32,
    pub height: u32,
    /// The color that plays the first move (`"black"` or `"white"`).
    pub initial_player: String,
    /// Handicap count, when the game is a handicap game.
    pub handicap: Option<u32>,
    /// Komi, when the server reports it.
    pub komi: Option<f64>,
    /// Server rules identifier (for example `japanese` or `chinese`).
    pub rules: Option<String>,
    /// Server time-control configuration.
    pub time_control: Option<TimeControl>,
    /// Setup stones placed before the first move (handicap / free placement).
    pub initial_black: Vec<String>,
    pub initial_white: Vec<String>,
    /// Server-confirmed moves as OGS coordinates (`"dd"`, pass `".."`).
    pub moves: Vec<String>,
    pub chat: Vec<OgsChatLine>,
    /// Server is asking players to mark/accept dead stones.
    pub stone_removal_mode: bool,
    /// Server-authoritative set of all removed stones (packed coordinates).
    pub removed_stones: String,
    /// Winner player id and outcome, populated when the game finishes.
    pub winner: Option<u64>,
    pub outcome: Option<String>,
    /// The most recent server-confirmed move coordinate (pass = `".."`).
    pub last_move: Option<String>,
    /// Whether the most recent move was our own pending-move confirmation.
    pub last_move_was_ours: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OgsChatLine {
    pub chat_id: Option<String>,
    pub username: String,
    pub body: String,
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
    /// Bumped whenever a connection is replaced or invalidated. Reader threads
    /// may only publish state while their captured generation remains current.
    generation: u64,
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
    transport_factory: Arc<dyn OgsWebSocketTransportFactory>,
    on_state_change: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl LiveOgsClient {
    pub fn new() -> Self {
        Self::with_parts(
            Box::new(KeyringOgsCredentialStore::new()),
            Box::new(UreqOgsRestFetch::new()),
        )
    }

    pub fn with_parts(store: Box<dyn OgsCredentialStore>, rest: Box<dyn OgsRestFetch>) -> Self {
        Self::with_parts_and_transport_factory(
            store,
            rest,
            Arc::new(TungsteniteOgsWebSocketTransportFactory),
        )
    }

    pub fn with_parts_and_transport_factory(
        store: Box<dyn OgsCredentialStore>,
        rest: Box<dyn OgsRestFetch>,
        transport_factory: Arc<dyn OgsWebSocketTransportFactory>,
    ) -> Self {
        Self {
            inner: Mutex::new(LiveOgsInner {
                snapshot: OgsClientSnapshot {
                    socket_status: OgsSocketStatus::Disconnected,
                    ..OgsClientSnapshot::default()
                },
                session: None,
                transport: None,
                next_request_id: 0,
                generation: 0,
                device_id: Uuid::new_v4().to_string(),
                competition: None,
            }),
            store,
            rest: Mutex::new(rest),
            transport_factory,
            on_state_change: Mutex::new(None),
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
    /// Sessions older than 30 days are cleared and must be re-established.
    pub fn restore_session(self: &Arc<Self>) -> Result<Value, String> {
        let Some(credentials) = self.store.load() else {
            return Err("No stored OGS session.".to_owned());
        };
        if credentials.jwt_token.is_empty() {
            return Err("Stored OGS session was empty.".to_owned());
        }
        const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if credentials
            .created_at
            .is_some_and(|created_at| now.saturating_sub(created_at) > SESSION_TTL_SECS)
        {
            self.store.clear();
            return Err("Stored OGS session has expired.".to_owned());
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
        // Keep the live session even when the OS credential store is unavailable,
        // but never hide the fact that this login will not survive a restart.
        let persistence_error = if !self.store.is_available() {
            Some("OGS 登录成功，但系统钥匙串不可用；重启后需要重新登录".to_owned())
        } else {
            self.store
                .save(&credentials)
                .err()
                .map(|error| format!("OGS 登录成功，但会话保存失败；重启后需要重新登录：{error}"))
        };

        let mut inner = self.inner.lock().unwrap();
        inner.session = Some(credentials);
        inner.snapshot.user = Some(user.clone());
        inner.snapshot.last_error = persistence_error;
        self.emit_state();
        Ok(user)
    }

    /// Best-effort server-side session revocation. OGS invalidates the session
    /// cookie (and associated JWT) via `GET /api/v0/logout`; without this the
    /// token remains valid on the server after a local logout.
    pub fn revoke_server_session(&self) -> Result<(), String> {
        let cookie_header = self
            .inner
            .lock()
            .unwrap()
            .session
            .as_ref()
            .and_then(|session| session.cookie_header.clone());
        let mut headers: Vec<(&str, &str)> = vec![("User-Agent", crate::ogs_rest::OGS_USER_AGENT)];
        if let Some(cookie) = cookie_header.as_deref() {
            headers.push(("Cookie", cookie));
        }
        let mut rest = self.rest.lock().unwrap();
        let response = rest.get_with_headers(
            &format!("{}/api/v0/logout", crate::ogs_rest::OGS_SERVER_URL),
            &headers,
        )?;
        if !(200..300).contains(&response.status) {
            return Err(format!("OGS logout returned status {}", response.status));
        }
        Ok(())
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

    /// Rejects a submission unless the logged-in user is a participant (black
    /// or white) in the given game. Spectators and unknown identities fail.
    fn require_participant(&self, game_id: u64) -> Result<(), String> {
        let inner = self.inner.lock().unwrap();
        let game = inner
            .snapshot
            .online_game
            .as_ref()
            .ok_or_else(|| "No active OGS game".to_owned())?;
        if game.game_id != game_id {
            return Err("OGS game id mismatch".to_owned());
        }
        let user_id = inner
            .snapshot
            .user
            .as_ref()
            .and_then(|user| user.get("id"))
            .and_then(|id| {
                id.as_u64()
                    .or_else(|| id.as_str().and_then(|s| s.parse::<u64>().ok()))
            })
            .unwrap_or(0);
        let username = inner
            .snapshot
            .user
            .as_ref()
            .and_then(|u| u.get("username"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let is_participant = (user_id != 0
            && (game.black_id == Some(user_id) || game.white_id == Some(user_id)))
            || (!username.is_empty()
                && (game.black_name.eq_ignore_ascii_case(username)
                    || game.white_name.eq_ignore_ascii_case(username)));
        if !is_participant {
            return Err("You are not a participant in this game".to_owned());
        }
        Ok(())
    }

    /// Validates the local prerequisites for a move/pass. Turn ownership is
    /// deliberately not rejected from cached state: OGS is authoritative and
    /// its realtime clock can briefly lag the move event. Sending the move lets
    /// the server accept a valid move instead of creating a false local lockout.
    fn require_my_turn(&self, game_id: u64) -> Result<(), String> {
        self.require_participant(game_id)?;
        let inner = self.inner.lock().unwrap();
        let game = inner.snapshot.online_game.as_ref().unwrap();
        if game.phase != "play" {
            return Err("Moves are only allowed during play".to_owned());
        }
        Ok(())
    }

    /// Rejects a dead-stone submission unless the user is a participant and the
    /// game is in the stone-removal phase.
    fn require_stone_removal(&self, game_id: u64) -> Result<(), String> {
        self.require_participant(game_id)?;
        let inner = self.inner.lock().unwrap();
        let game = inner.snapshot.online_game.as_ref().unwrap();
        if game.phase != "stone removal" {
            return Err("Dead-stone marking is only allowed during stone removal".to_owned());
        }
        Ok(())
    }

    /// Submits a move. `vertex` is an OGS coordinate string (`Some("dd")`); a
    /// pass is `None` (encoded as `".."`).
    pub fn play_move(&self, game_id: u64, vertex: Option<String>) -> Result<(), String> {
        self.require_my_turn(game_id)?;
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
        self.require_participant(game_id)?;
        self.send_event("game/resign", &serde_json::json!({"game_id": game_id}))
    }

    pub fn set_removed_stones(
        &self,
        game_id: u64,
        stones: &str,
        removed: bool,
    ) -> Result<(), String> {
        self.require_stone_removal(game_id)?;
        self.send_event(
            "game/removed_stones/set",
            &serde_json::json!({"game_id": game_id, "removed": removed, "stones": stones}),
        )
    }

    pub fn accept_removed_stones(&self, game_id: u64, stones: &str) -> Result<(), String> {
        self.require_stone_removal(game_id)?;
        self.send_event(
            "game/removed_stones/accept",
            &serde_json::json!({
                "game_id": game_id,
                "stones": stones,
                "strict_seki_mode": false,
            }),
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
        // A fresh login supersedes every prior socket before any network work.
        // A reader that wakes after this point is generation-gated and inert.
        let (generation, previous_transport, device_id) = {
            let mut inner = self.inner.lock().unwrap();
            inner.generation = inner.generation.wrapping_add(1);
            let generation = inner.generation;
            let previous_transport = inner.transport.take();
            inner.snapshot.socket_status = OgsSocketStatus::Connecting;
            (generation, previous_transport, inner.device_id.clone())
        };
        if let Some(transport) = previous_transport {
            transport.close();
        }
        self.emit_state();

        let transport = self.transport_factory.create();
        if let Err(error) = transport.connect(OGS_SOCKET_URL) {
            self.set_connection_error(generation, error.clone());
            return Err(error);
        }

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
                    OgsIncoming::Response { id, error, .. } if id == request_id => {
                        if let Some(error) = error {
                            transport.close();
                            let message = format!("OGS authentication failed: {error}");
                            self.set_connection_error(generation, message.clone());
                            return Err(message);
                        }
                        break;
                    }
                    OgsIncoming::Response { .. } | OgsIncoming::Event { .. } => {}
                }
            }
        }

        {
            let mut inner = self.inner.lock().unwrap();
            if inner.generation != generation {
                transport.close();
                return Err("OGS connection was superseded".to_owned());
            }
            inner.transport = Some(Arc::clone(&transport));
            inner.snapshot.socket_status = OgsSocketStatus::Authenticated;
        }
        self.spawn_reader(generation, transport);
        self.emit_state();
        Ok(())
    }

    fn spawn_reader(self: &Arc<Self>, generation: u64, transport: Arc<dyn OgsWebSocketTransport>) {
        let client = Arc::clone(self);
        std::thread::Builder::new()
            .name("ogs-client-reader".to_owned())
            .spawn(move || {
                reader_loop(client, generation, transport);
            })
            .ok();
    }

    fn set_connection_error(&self, generation: u64, message: String) {
        let mut inner = self.inner.lock().unwrap();
        if inner.generation == generation {
            inner.snapshot.socket_status = OgsSocketStatus::Error;
            inner.snapshot.last_error = Some(message);
            if let Some(game) = inner.snapshot.online_game.as_mut() {
                game.connected = false;
            }
        }
        drop(inner);
        self.emit_state();
    }

    fn reconnect_after_disconnect(self: &Arc<Self>, jwt: String, game_id: Option<u64>) {
        let client = Arc::clone(self);
        std::thread::Builder::new()
            .name("ogs-reconnect".to_owned())
            .spawn(move || {
                for attempt in 0.. {
                    std::thread::sleep(ogs_reconnect_delay(attempt));
                    let still_logged_in = client
                        .inner
                        .lock()
                        .unwrap()
                        .session
                        .as_ref()
                        .is_some_and(|session| session.jwt_token == jwt);
                    if !still_logged_in {
                        return;
                    }
                    if client.connect_and_authenticate(&jwt).is_ok() {
                        if let Some(game_id) = game_id {
                            // Re-subscribe without replacing the preserved board;
                            // the following gamedata event refreshes it authoritatively.
                            let _ = client.send_event(
                                "game/connect",
                                &serde_json::json!({"game_id": game_id, "chat": true}),
                            );
                        }
                        return;
                    }
                }
            })
            .ok();
    }

    fn stop_reader(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.generation = inner.generation.wrapping_add(1);
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
            // A cancelled matchmaking round (either side declined / timed out)
            // returns to Idle; otherwise the UI would stay "matching" forever.
            "automatch/cancel" => {
                let mut inner = self.inner.lock().unwrap();
                inner.snapshot.matchmaking_status = OgsMatchmakingStatus::Idle;
                inner.snapshot.automatch_uuid = None;
                inner.snapshot.matched_game_id = None;
            }
            "net/pong" | "user/state" | "active_game" | "notification" => {}
            "net/ping" => {
                let _ = self.send_event("net/pong", payload);
            }
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
                    "chat" => self.apply_chat(game_id, payload),
                    "chat/remove" => self.apply_chat_remove(game_id, payload),
                    "error" => self.apply_game_error(game_id, payload),
                    "phase" => self.apply_phase(game_id, payload),
                    "removed_stones" => self.apply_removed_stones(game_id, payload),
                    "removed_stones_accepted" => {
                        self.apply_removed_stones_accepted(game_id, payload)
                    }
                    "latency" => {}
                    _ => {}
                }
            }
        }
        self.emit_state();
    }

    fn apply_gamedata(&self, game_id: u64, payload: &Value) {
        let moves = parse_ogs_moves(payload);
        let move_number = if moves.is_empty() {
            payload
                .get("move_number")
                .and_then(value_as_u32)
                .unwrap_or(0)
        } else {
            // The move list is the authoritative count in a complete OGS
            // snapshot. Some server payloads carry an auxiliary move_number
            // cursor with different semantics.
            moves.len() as u32
        };
        let black_name = player_name(payload, "black");
        let white_name = player_name(payload, "white");
        let black_id = player_id_for_color(payload, "black");
        let white_id = player_id_for_color(payload, "white");
        let initial_player = payload
            .get("initial_player")
            .and_then(Value::as_str)
            .unwrap_or("black")
            .to_owned();
        let next_player = player_to_move(payload, move_number, &initial_player);
        let clock = parse_clock(payload.get("clock"), black_id, white_id);
        let phase = payload
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let width = payload.get("width").and_then(Value::as_u64).unwrap_or(19) as u32;
        let height = payload.get("height").and_then(Value::as_u64).unwrap_or(19) as u32;
        let stone_removal_mode = payload
            .pointer("/clock/stone_removal_mode")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let handicap = payload
            .get("handicap")
            .and_then(Value::as_u64)
            .map(|n| n as u32);
        let komi = payload.get("komi").and_then(value_as_f64);
        let rules = payload
            .get("rules")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let time_control = parse_ogs_time_control(payload.get("time_control"));
        let initial_black = payload
            .pointer("/initial_state/black")
            .and_then(Value::as_str)
            .map(decode_packed_coords)
            .unwrap_or_default();
        let initial_white = payload
            .pointer("/initial_state/white")
            .and_then(Value::as_str)
            .map(decode_packed_coords)
            .unwrap_or_default();

        // OGS can replay a complete game snapshot after an incremental move.
        // Never let a delayed snapshot rewind the confirmed board or turn.
        if self
            .inner
            .lock()
            .unwrap()
            .snapshot
            .online_game
            .as_ref()
            .is_some_and(|game| game.game_id == game_id && move_number < game.move_number)
        {
            return;
        }

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
            black_id,
            white_id,
            phase,
            pending_move: false,
            width,
            height,
            initial_player,
            handicap,
            komi,
            rules,
            time_control,
            initial_black,
            initial_white,
            last_move: moves.last().cloned(),
            last_move_was_ours: false,
            moves,
            chat: Vec::new(),
            stone_removal_mode,
            removed_stones: String::new(),
            winner: None,
            outcome: None,
        });
        if let Some(update) = update {
            inner.competition = OgsCompetitionSession::new(game_id, update).ok();
        }
    }

    fn apply_move(&self, game_id: u64, payload: &Value) {
        // OGS reports the move number after applying this move. The official
        // client verifies that its current position is move_number - 1 first.
        let move_number = payload.get("move_number").and_then(value_as_u32);
        let mut inner = self.inner.lock().unwrap();
        let Some(game) = inner.snapshot.online_game.as_mut() else {
            return;
        };
        if game.game_id != game_id {
            return;
        }
        // The `move` field may be a string ("dd") or an array ([x, y]); reuse
        // the same tolerant parser as gamedata so both forms project correctly.
        let move_string = payload
            .get("move")
            .and_then(encode_ogs_move_item)
            .or_else(|| {
                payload
                    .get("move")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            });
        // Incremental events can be duplicated or arrive after a complete
        // snapshot. In OGS, move_number is the resulting number of moves.
        if move_number.is_some_and(|number| number <= game.move_number)
            || (move_number.is_none() && move_string.as_ref() == game.moves.last())
        {
            return;
        }
        let was_ours = game.pending_move;
        let next_player_from_server =
            player_color_from_move_payload(payload, game.black_id, game.white_id);
        game.move_number = move_number.unwrap_or_else(|| game.move_number.saturating_add(1));
        game.pending_move = false;
        game.last_move = move_string.clone();
        game.last_move_was_ours = was_ours;
        if let Some(coord) =
            move_string.filter(|coord| !game.moves.last().is_some_and(|last| last == coord))
        {
            game.moves.push(coord);
        }
        // Prefer the server-authoritative next player. Only use local parity as
        // a fallback for old OGS frames that omit current_player entirely.
        let expected_next_player = next_player_for_position(game.move_number, &game.initial_player);
        game.next_player = next_player_from_server
            .filter(|player| Some(*player) == expected_next_player)
            .or(expected_next_player);
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
        let mut inner = self.inner.lock().unwrap();
        let Some(game) = inner.snapshot.online_game.as_mut() else {
            return;
        };
        if game.game_id != game_id {
            return;
        }
        let clock = parse_clock(Some(payload), game.black_id, game.white_id);
        let expected_next_player = next_player_for_position(game.move_number, &game.initial_player);
        if let Some(active) = clock.as_ref().and_then(|c| c.active_color)
            && Some(active) == expected_next_player
        {
            game.next_player = Some(active);
        } else if expected_next_player.is_some() {
            // Clock frames can be delivered around the move frame. Do not let a
            // stale pre-move clock overwrite the turn derived from confirmed
            // moves and the game's initial player.
            game.next_player = expected_next_player;
        }
        game.clock = clock;
        game.stone_removal_mode = payload
            .get("stone_removal_mode")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }

    fn apply_chat(&self, game_id: u64, payload: &Value) {
        let line = payload.get("line").unwrap_or(payload);
        let chat_id = line
            .get("chat_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let username = line
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .chars()
            .take(MAX_CHAT_USERNAME_CHARS)
            .collect::<String>();
        let body = line
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(MAX_CHAT_BODY_CHARS)
            .collect::<String>();
        if body.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        let Some(game) = inner.snapshot.online_game.as_mut() else {
            return;
        };
        if game.game_id != game_id {
            return;
        }
        game.chat.push(OgsChatLine {
            chat_id,
            username,
            body,
        });
        // Bound the retained history so a chat flood cannot grow memory without
        // limit; the UI only renders the most recent lines anyway.
        if game.chat.len() > MAX_CHAT_LINES {
            let excess = game.chat.len() - MAX_CHAT_LINES;
            game.chat.drain(..excess);
        }
    }

    fn apply_chat_remove(&self, game_id: u64, payload: &Value) {
        let chat_ids: Vec<String> = payload
            .get("chat_ids")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if chat_ids.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        let Some(game) = inner.snapshot.online_game.as_mut() else {
            return;
        };
        if game.game_id != game_id {
            return;
        }
        game.chat.retain(|line| {
            !line
                .chat_id
                .as_ref()
                .is_some_and(|id| chat_ids.contains(id))
        });
    }

    fn apply_game_error(&self, game_id: u64, payload: &Value) {
        let message = payload.as_str().unwrap_or("OGS game error").to_owned();
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.last_error = Some(message);
        // A game that never connected is a phantom placeholder; drop it so the
        // UI does not show an empty board with no feedback.
        if inner
            .snapshot
            .online_game
            .as_ref()
            .is_some_and(|game| game.game_id == game_id && !game.connected)
        {
            inner.snapshot.online_game = None;
            inner.competition = None;
        }
    }

    fn apply_phase(&self, game_id: u64, payload: &Value) {
        let phase = payload.as_str().unwrap_or("").to_owned();
        let mut inner = self.inner.lock().unwrap();
        if let Some(game) = inner.snapshot.online_game.as_mut()
            && game.game_id == game_id
        {
            game.phase = phase;
        }
    }

    fn apply_removed_stones(&self, game_id: u64, payload: &Value) {
        // The server broadcasts the authoritative set of all removed stones.
        let all_removed = payload
            .get("all_removed")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut inner = self.inner.lock().unwrap();
        if let Some(game) = inner.snapshot.online_game.as_mut()
            && game.game_id == game_id
        {
            game.removed_stones = all_removed.to_owned();
        }
    }

    fn apply_removed_stones_accepted(&self, game_id: u64, payload: &Value) {
        let phase = payload
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("finished")
            .to_owned();
        let winner = payload.get("winner").and_then(Value::as_u64);
        let outcome = payload
            .get("outcome")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let mut inner = self.inner.lock().unwrap();
        if let Some(game) = inner.snapshot.online_game.as_mut()
            && game.game_id == game_id
        {
            game.phase = phase;
            game.winner = winner;
            game.outcome = outcome;
        }
    }
}

impl Default for LiveOgsClient {
    fn default() -> Self {
        Self::new()
    }
}

fn reader_loop(
    client: Arc<LiveOgsClient>,
    generation: u64,
    transport: Arc<dyn OgsWebSocketTransport>,
) {
    let mut last_ping = std::time::Instant::now();
    loop {
        if client.inner.lock().unwrap().generation != generation {
            return;
        }
        let mut failed = None;
        match transport.recv_text(Duration::from_secs(1)) {
            Ok(Some(text)) => {
                if let Ok(incoming) = decode_incoming(&text)
                    && client.inner.lock().unwrap().generation == generation
                {
                    client.handle_incoming(incoming);
                }
            }
            Ok(None) => {}
            Err(error) => {
                failed = Some(format!("OGS socket read failed: {error}"));
            }
        }

        // Heartbeats must be based on elapsed wall time, not on receiving an
        // empty read. A busy game can deliver a frame every second forever and
        // otherwise starve this keep-alive indefinitely.
        if failed.is_none() && last_ping.elapsed() >= ogs_heartbeat_interval() {
            if let Err(error) = transport.send_text(&encode_event(
                "net/ping",
                &serde_json::json!({"client": unix_time_millis()}),
            )) {
                failed = Some(format!("OGS socket heartbeat failed: {error}"));
            } else {
                last_ping = std::time::Instant::now();
            }
        }

        if let Some(error) = failed {
            transport.close();
            client.set_connection_error(generation, error);
            let reconnect = {
                let inner = client.inner.lock().unwrap();
                if inner.generation == generation {
                    inner.session.as_ref().map(|session| {
                        (
                            session.jwt_token.clone(),
                            inner.snapshot.online_game.as_ref().map(|game| game.game_id),
                        )
                    })
                } else {
                    None
                }
            };
            if let Some((jwt, game_id)) = reconnect {
                client.reconnect_after_disconnect(jwt, game_id);
            }
            return;
        }
    }
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

// -- pure payload parsers ----------------------------------------------------

fn value_as_u32(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .or_else(|| value.as_str().and_then(|text| text.parse::<u32>().ok()))
}

fn player_id(payload: &Value, pointer: &str) -> Option<u64> {
    payload.pointer(pointer).and_then(|id| {
        id.as_u64()
            .or_else(|| id.as_str().and_then(|text| text.parse::<u64>().ok()))
    })
}

fn player_id_for_color(payload: &Value, color: &str) -> Option<u64> {
    player_id(payload, &format!("/players/{color}/id"))
        .or_else(|| player_id(payload, &format!("/{color}/id")))
        .or_else(|| player_id(payload, &format!("/{color}_player_id")))
        .or_else(|| player_id(payload, &format!("/{color}")))
}

fn next_player_for_position(move_number: u32, initial_player: &str) -> Option<Color> {
    let first = if initial_player.eq_ignore_ascii_case("white") {
        Color::White
    } else {
        Color::Black
    };
    if move_number.is_multiple_of(2) {
        Some(first)
    } else {
        Some(first.opponent())
    }
}

fn color_for_player_value(
    value: &Value,
    black_id: Option<u64>,
    white_id: Option<u64>,
) -> Option<Color> {
    if let Some(color) = value.as_str() {
        if color.eq_ignore_ascii_case("black") || color.eq_ignore_ascii_case("b") {
            return Some(Color::Black);
        }
        if color.eq_ignore_ascii_case("white") || color.eq_ignore_ascii_case("w") {
            return Some(Color::White);
        }
    }
    let id = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))?;
    if Some(id) == black_id {
        Some(Color::Black)
    } else if Some(id) == white_id {
        Some(Color::White)
    } else {
        None
    }
}

fn player_color_from_move_payload(
    payload: &Value,
    black_id: Option<u64>,
    white_id: Option<u64>,
) -> Option<Color> {
    payload
        .get("next_player")
        .or_else(|| payload.get("current_player"))
        .or_else(|| payload.pointer("/clock/current_player"))
        .and_then(|value| color_for_player_value(value, black_id, white_id))
}

fn player_to_move(payload: &Value, move_number: u32, initial_player: &str) -> Option<Color> {
    let black_id = player_id_for_color(payload, "black");
    let white_id = player_id_for_color(payload, "white");
    let current_player = player_id(payload, "/clock/current_player");
    match current_player {
        Some(id) if Some(id) == black_id => Some(Color::Black),
        Some(id) if Some(id) == white_id => Some(Color::White),
        _ => next_player_for_position(move_number, initial_player),
    }
}

fn player_name(payload: &Value, color: &str) -> String {
    [
        format!("/players/{color}/username"),
        format!("/players/{color}/name"),
        format!("/{color}/username"),
        format!("/{color}/name"),
    ]
    .iter()
    .find_map(|pointer| payload.pointer(pointer).and_then(Value::as_str))
    .unwrap_or(color)
    .to_owned()
}

/// Decodes OGS `moves` into coordinate strings (`"dd"`, pass `".."`). Items may
/// be `[x, y]`, `[x, y, n]`, `[move_string, n]`, or a plain string.
/// Decodes an OGS packed coordinate string (`"ddpp"` = `dd` then `pp`, with
/// `..` for a pass) into individual two-character coordinates.
fn decode_packed_coords(text: &str) -> Vec<String> {
    text.as_bytes()
        .chunks(2)
        .filter(|chunk| {
            chunk.len() == 2
                && chunk
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || *byte == b'.')
        })
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect()
}

fn parse_ogs_moves(payload: &Value) -> Vec<String> {
    match payload.get("moves") {
        Some(Value::Array(moves)) => moves.iter().filter_map(encode_ogs_move_item).collect(),
        // OGS may pack moves into a single string of two-character coordinates
        // (e.g. `"ddpp"` = `dd` then `pp`, with `..` for a pass).
        Some(Value::String(text)) => decode_packed_coords(text),
        _ => Vec::new(),
    }
}

fn encode_ogs_move_item(item: &Value) -> Option<String> {
    match item {
        Value::String(text) => {
            let coord = text.chars().take(2).collect::<String>();
            (coord.len() == 2 && coord.chars().all(|c| c.is_ascii_lowercase() || c == '.'))
                .then_some(coord)
        }
        Value::Array(array) if !array.is_empty() => {
            if let (Some(x), Some(y)) = (array[0].as_i64(), array[1].as_i64()) {
                encode_ogs_coordinates(x, y)
            } else if let Some(coord) = array[0].as_str() {
                encode_ogs_move_item(&Value::String(coord.to_owned()))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn encode_ogs_coordinates(x: i64, y: i64) -> Option<String> {
    if x == -1 && y == -1 {
        return Some("..".to_owned());
    }
    if !(0..=25).contains(&x) || !(0..=25).contains(&y) {
        return None;
    }
    let column = char::from_u32((b'a' + x as u8) as u32)?;
    let row = char::from_u32((b'a' + y as u8) as u32)?;
    Some(format!("{column}{row}"))
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
}

fn time_control_seconds(value: Option<&Value>) -> Option<u64> {
    let seconds = value_as_f64(value?)?;
    seconds
        .is_finite()
        .then_some(seconds.max(0.0).round() as u64)
}

fn parse_ogs_time_control(value: Option<&Value>) -> Option<TimeControl> {
    let object = value?.as_object()?;
    match object
        .get("system")
        .and_then(Value::as_str)?
        .to_ascii_lowercase()
        .as_str()
    {
        "none" => Some(TimeControl::None),
        "absolute" => Some(TimeControl::Absolute {
            main_time_secs: time_control_seconds(object.get("total_time"))?,
        }),
        "fischer" => Some(TimeControl::Fischer {
            main_time_secs: time_control_seconds(object.get("initial_time"))?,
            increment_secs: time_control_seconds(object.get("time_increment"))?,
        }),
        "byoyomi" => Some(TimeControl::ByoYomi {
            main_time_secs: time_control_seconds(object.get("main_time"))?,
            period_time_secs: time_control_seconds(object.get("period_time"))?,
            periods: object.get("periods").and_then(value_as_u32)?,
        }),
        // These systems have no direct representation in the domain clock yet.
        // Keep the server clock visible, but do not mislabel it as another system.
        "simple" | "canadian" => None,
        _ => None,
    }
}

fn seconds_to_duration(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds >= 0.0 {
        // `from_secs_f64` panics on values beyond `Duration::MAX`; a malicious
        // or broken clock payload must degrade to zero instead of killing the
        // reader thread.
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO)
    } else {
        Duration::ZERO
    }
}

fn clock_duration(value: Option<&Value>, field: &str) -> Duration {
    let Some(value) = value else {
        return Duration::ZERO;
    };
    if let Some(object) = value.as_object() {
        return seconds_to_duration(
            object
                .get(field)
                .or_else(|| object.get("time"))
                .or_else(|| object.get("period_time"))
                .and_then(value_as_f64)
                .unwrap_or(0.0),
        );
    }
    // AdHoc simple-time clocks use milliseconds directly for numeric values.
    value
        .as_f64()
        .filter(|millis| millis.is_finite() && *millis >= 0.0)
        .map(|millis| Duration::from_millis(millis.round() as u64))
        .unwrap_or(Duration::ZERO)
}

fn clock_periods(value: Option<&Value>) -> u32 {
    value
        .and_then(|value| value.get("periods").or_else(|| value.get("periods_left")))
        .and_then(value_as_u32)
        .unwrap_or(0)
}

fn parse_clock(
    clock: Option<&Value>,
    black_id: Option<u64>,
    white_id: Option<u64>,
) -> Option<OgsServerClock> {
    let clock = clock?;
    let black = clock.get("black_time");
    let white = clock.get("white_time");
    let clock_black_id = player_id(clock, "/black_player_id").or(black_id);
    let clock_white_id = player_id(clock, "/white_player_id").or(white_id);
    let active_color = clock
        .get("current_player")
        .and_then(|value| color_for_player_value(value, clock_black_id, clock_white_id));
    let paused = clock
        .pointer("/pause/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(OgsServerClock {
        black_main_remaining: clock_duration(black, "thinking_time"),
        white_main_remaining: clock_duration(white, "thinking_time"),
        black_byo_yomi_remaining: clock_duration(black, "period_time_left"),
        white_byo_yomi_remaining: clock_duration(white, "period_time_left"),
        black_periods: clock_periods(black),
        white_periods: clock_periods(white),
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
    use crate::ogs_rest::{OgsHttpResponse, OgsLoginResult};

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
        assert_eq!(player_to_move(&payload, 2, "black"), Some(Color::White));
        assert_eq!(player_name(&payload, "black"), "Black");
        let clock = parse_clock(payload.get("clock"), Some(7), Some(8)).unwrap();
        assert_eq!(clock.black_main_remaining, Duration::from_secs(500));
        assert_eq!(clock.white_periods, 5);
        assert_eq!(clock.active_color, Some(Color::White));
    }

    #[test]
    fn decodes_packed_coordinate_strings() {
        assert_eq!(decode_packed_coords("ddpp"), vec!["dd", "pp"]);
        assert_eq!(decode_packed_coords("dd.."), vec!["dd", ".."]);
        assert_eq!(decode_packed_coords(""), Vec::<String>::new());
    }

    #[test]
    fn gamedata_parses_handicap_initial_state_and_komi() {
        let client = LiveOgsClient::with_parts(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
        );
        let payload = serde_json::json!({
            "width": 9,
            "height": 13,
            "phase": "play",
            "initial_player": "white",
            "handicap": 2,
            "komi": 0.5,
            "rules": "japanese",
            "time_control": {
                "system": "byoyomi",
                "main_time": 600,
                "period_time": 30,
                "periods": 5
            },
            "initial_state": {"black": "ddpp", "white": ""},
            "moves": [[3, 3, 1]],
            "players": {
                "black": {"id": 7, "username": "Black"},
                "white": {"id": 8, "username": "White"}
            }
        });
        client.apply_gamedata(1, &payload);
        let game = client.snapshot().online_game.unwrap();
        assert_eq!(game.width, 9);
        assert_eq!(game.height, 13);
        assert_eq!(game.initial_player, "white");
        assert_eq!(game.handicap, Some(2));
        assert_eq!(game.komi, Some(0.5));
        assert_eq!(game.rules.as_deref(), Some("japanese"));
        assert_eq!(
            game.time_control,
            Some(TimeControl::ByoYomi {
                main_time_secs: 600,
                period_time_secs: 30,
                periods: 5,
            })
        );
        assert_eq!(game.initial_black, vec!["dd", "pp"]);
        assert!(game.initial_white.is_empty());
    }

    #[test]
    fn infers_next_player_from_move_parity() {
        let payload = serde_json::json!({"moves": [[3, 3, 1]]});
        assert_eq!(player_to_move(&payload, 1, "black"), Some(Color::White));
        assert_eq!(player_to_move(&payload, 2, "black"), Some(Color::Black));
    }

    #[test]
    fn infers_next_player_from_initial_player_when_white_starts() {
        let payload = serde_json::json!({
            "initial_player": "white",
            "moves": [[3, 3, 1]],
        });
        assert_eq!(player_to_move(&payload, 1, "white"), Some(Color::Black));
        assert_eq!(player_to_move(&payload, 2, "white"), Some(Color::White));
    }

    #[test]
    fn clock_event_maps_string_current_player_ids_from_game_state() {
        let client = client_with_game(7, 7, 8);
        client
            .inner
            .lock()
            .unwrap()
            .snapshot
            .online_game
            .as_mut()
            .unwrap()
            .move_number = 1;
        client.apply_clock(
            1,
            &serde_json::json!({
                "current_player": "8",
                "black_time": {"thinking_time": 100.0},
                "white_time": {"thinking_time": 100.0}
            }),
        );
        assert_eq!(
            client.snapshot().online_game.unwrap().next_player,
            Some(Color::White)
        );
    }

    #[test]
    fn stale_clock_after_opponent_move_does_not_rewind_my_turn() {
        let client = client_with_game(8, 7, 8);
        client.apply_move(1, &serde_json::json!({"move": "dd", "move_number": 1}));
        // This is a pre-move clock frame arriving late; it must not overwrite
        // the turn derived from the confirmed one-move position.
        client.apply_clock(
            1,
            &serde_json::json!({
                "current_player": 7,
                "black_time": {"thinking_time": 100.0},
                "white_time": {"thinking_time": 100.0}
            }),
        );
        assert!(client.require_my_turn(1).is_ok());
        assert_eq!(
            client.snapshot().online_game.unwrap().next_player,
            Some(Color::White)
        );
    }

    #[test]
    fn stale_gamedata_does_not_rewind_confirmed_moves_or_turn() {
        let client = client_with_game(7, 7, 8);
        client.apply_move(1, &serde_json::json!({"move": "dd", "move_number": 1}));
        client.apply_gamedata(
            1,
            &serde_json::json!({
                "phase": "play",
                "move_number": 0,
                "moves": [],
                "players": {
                    "black": {"id": 7, "username": "Black"},
                    "white": {"id": 8, "username": "White"}
                }
            }),
        );
        let game = client.snapshot().online_game.unwrap();
        assert_eq!(game.move_number, 1);
        assert_eq!(game.moves, vec!["dd"]);
        assert_eq!(game.next_player, Some(Color::White));
    }

    #[test]
    fn move_event_uses_server_current_player_over_local_flip() {
        let client = client_with_game(7, 7, 8);
        client.apply_move(
            1,
            &serde_json::json!({
                "move": "dd",
                "move_number": 1,
                "clock": {"current_player": "8"}
            }),
        );
        assert_eq!(
            client.snapshot().online_game.unwrap().next_player,
            Some(Color::White)
        );
    }

    #[test]
    fn duplicate_move_event_does_not_flip_turn_again() {
        let client = client_with_game(7, 7, 8);
        let move_event = serde_json::json!({"move": "dd", "move_number": 1});
        client.apply_move(1, &move_event);
        client.apply_move(1, &move_event);
        let game = client.snapshot().online_game.unwrap();
        assert_eq!(game.move_number, 1);
        assert_eq!(game.moves, vec!["dd"]);
        assert_eq!(game.next_player, Some(Color::White));
    }

    #[test]
    fn move_submission_uses_official_ogs_payload_shape() {
        let transport = Arc::new(ScriptedTransport::new(Vec::new()));
        let client = client_with_game(7, 7, 8);
        {
            let mut inner = client.inner.lock().unwrap();
            inner.transport = Some(transport.clone());
            let game = inner.snapshot.online_game.as_mut().unwrap();
            game.move_number = 4;
            game.next_player = Some(Color::Black);
        }
        client
            .play_move(1, Some("dd".to_owned()))
            .expect("move submission succeeds");
        let sent = transport.sent.lock().unwrap();
        let request: Value = serde_json::from_str(&sent[0]).expect("valid OGS frame");
        assert_eq!(request[0], "game/move");
        assert_eq!(request[1]["game_id"], 1);
        assert_eq!(request[1]["move"], "dd");
        assert_eq!(request[1].get("move_number"), None);
    }

    #[test]
    fn apply_move_accepts_array_and_string_coordinates() {
        let client = LiveOgsClient::with_parts(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
        );
        // Establish an online game via gamedata (empty board, black to move).
        let gamedata = serde_json::json!({
            "width": 19,
            "height": 19,
            "phase": "play",
            "move_number": 0,
            "players": {
                "black": {"id": 7, "username": "Black"},
                "white": {"id": 8, "username": "White"}
            },
            "clock": {"current_player": 7, "black_player_id": 7, "white_player_id": 8}
        });
        client.apply_gamedata(1, &gamedata);
        // The server confirms a move using the array coordinate form.
        client.apply_move(1, &serde_json::json!({"move": [3, 3], "move_number": 1}));
        let game = client.snapshot().online_game.unwrap();
        assert_eq!(game.moves, vec!["dd"]);
        assert_eq!(game.next_player, Some(Color::White));

        // A string coordinate must project just as well.
        client.apply_move(1, &serde_json::json!({"move": "pp", "move_number": 2}));
        let game = client.snapshot().online_game.unwrap();
        assert_eq!(game.moves, vec!["dd", "pp"]);
    }

    #[test]
    fn parses_ogs_time_control_variants() {
        assert_eq!(
            parse_ogs_time_control(Some(&serde_json::json!({
                "system": "absolute",
                "total_time": 900
            }))),
            Some(TimeControl::Absolute {
                main_time_secs: 900
            })
        );
        assert_eq!(
            parse_ogs_time_control(Some(&serde_json::json!({
                "system": "fischer",
                "initial_time": 600,
                "time_increment": 10
            }))),
            Some(TimeControl::Fischer {
                main_time_secs: 600,
                increment_secs: 10
            })
        );
        assert_eq!(
            parse_ogs_time_control(Some(&serde_json::json!({
                "system": "none"
            }))),
            Some(TimeControl::None)
        );
    }

    #[test]
    fn numeric_ogs_clock_values_are_milliseconds() {
        let clock = parse_clock(
            Some(&serde_json::json!({
                "current_player": 7,
                "black_time": 90_000,
                "white_time": 120_000
            })),
            Some(7),
            Some(8),
        )
        .unwrap();
        assert_eq!(clock.black_main_remaining, Duration::from_secs(90));
        assert_eq!(clock.white_main_remaining, Duration::from_secs(120));
    }

    #[test]
    fn oversized_clock_values_degrade_to_zero_instead_of_panicking() {
        assert_eq!(seconds_to_duration(1e300), Duration::ZERO);
        assert_eq!(seconds_to_duration(f64::INFINITY), Duration::ZERO);
        assert_eq!(seconds_to_duration(f64::NAN), Duration::ZERO);
        assert_eq!(seconds_to_duration(-1.0), Duration::ZERO);
        assert_eq!(seconds_to_duration(30.0), Duration::from_secs(30));
    }

    #[test]
    fn parses_ogs_moves_in_array_forms() {
        let payload = serde_json::json!({"moves": [[3, 3], [15, 15, 2], "pp", [-1, -1]]});
        assert_eq!(parse_ogs_moves(&payload), vec!["dd", "pp", "pp", ".."]);
    }

    #[test]
    fn parses_string_packed_ogs_moves() {
        let payload = serde_json::json!({"moves": "ddpp"});
        assert_eq!(parse_ogs_moves(&payload), vec!["dd", "pp"]);
        let pass_payload = serde_json::json!({"moves": "dd.."});
        assert_eq!(parse_ogs_moves(&pass_payload), vec!["dd", ".."]);
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

    #[test]
    fn restore_session_rejects_credentials_older_than_thirty_days() {
        let store = MemoryOgsCredentialStore::available();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // 31 days old: just past the 30-day retention window.
        let stale_created_at = now - (31 * 24 * 60 * 60);
        store
            .save(&OgsCredentials {
                server_url: "https://online-go.com".to_owned(),
                jwt_token: "stale-jwt".to_owned(),
                cookie_header: None,
                user: Some(serde_json::json!({"id": 7})),
                created_at: Some(stale_created_at),
            })
            .expect("save succeeds");
        let client = Arc::new(LiveOgsClient::with_parts(
            Box::new(store),
            Box::new(TestRestFetch),
        ));
        let result = client.restore_session();
        assert!(result.is_err(), "stale session must be rejected");
        assert!(client.snapshot().user.is_none());
    }

    struct TestRestFetch;
    impl OgsRestFetch for TestRestFetch {
        fn get(&mut self, _url: &str) -> Result<OgsHttpResponse, String> {
            Err("unused".to_owned())
        }
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<OgsHttpResponse, String> {
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

    /// A scripted transport that replays a queue of `recv_text` results and
    /// records everything it sends, so lifecycle tests stay fully offline.
    struct ScriptedTransport {
        recv: Mutex<std::collections::VecDeque<Result<Option<String>, String>>>,
        sent: Mutex<Vec<String>>,
        closed: std::sync::atomic::AtomicBool,
        continuous: bool,
    }

    impl ScriptedTransport {
        fn new(recv: Vec<Result<Option<String>, String>>) -> Self {
            Self {
                recv: Mutex::new(recv.into()),
                sent: Mutex::new(Vec::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
                continuous: false,
            }
        }

        fn continuous() -> Self {
            Self {
                recv: Mutex::new(std::collections::VecDeque::new()),
                sent: Mutex::new(Vec::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
                continuous: true,
            }
        }
    }

    impl OgsWebSocketTransport for ScriptedTransport {
        fn connect(&self, _url: &str) -> Result<(), String> {
            Ok(())
        }
        fn send_text(&self, message: &str) -> Result<(), String> {
            self.sent.lock().unwrap().push(message.to_owned());
            Ok(())
        }
        fn recv_text(&self, _timeout: Duration) -> Result<Option<String>, String> {
            if self.continuous {
                std::thread::sleep(Duration::from_millis(1));
                return Ok(Some("[\"net/pong\",{}]".to_owned()));
            }
            match self.recv.lock().unwrap().pop_front() {
                Some(result) => result,
                None => {
                    // Simulate a quiet socket without hot-spinning the reader.
                    std::thread::sleep(Duration::from_millis(5));
                    Ok(None)
                }
            }
        }
        fn close(&self) {
            self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    struct ScriptedTransportFactory {
        transports: Mutex<std::collections::VecDeque<Arc<dyn OgsWebSocketTransport>>>,
    }

    impl ScriptedTransportFactory {
        fn new(transports: Vec<Arc<dyn OgsWebSocketTransport>>) -> Self {
            Self {
                transports: Mutex::new(transports.into()),
            }
        }
    }

    impl OgsWebSocketTransportFactory for ScriptedTransportFactory {
        fn create(&self) -> Arc<dyn OgsWebSocketTransport> {
            self.transports
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted transport exhausted")
        }
    }

    fn auth_reply(id: u64) -> String {
        serde_json::json!([id, {"jwt": "ok"}]).to_string()
    }

    fn auth_error_reply(id: u64) -> String {
        serde_json::json!([id, null, "invalid token"]).to_string()
    }

    #[test]
    fn login_keeps_live_session_but_reports_unavailable_persistence() {
        let transport = Arc::new(ScriptedTransport::new(vec![Ok(Some(auth_reply(1)))]));
        let factory = Arc::new(ScriptedTransportFactory::new(vec![transport]));
        let client = Arc::new(LiveOgsClient::with_parts_and_transport_factory(
            Box::new(MemoryOgsCredentialStore::unavailable()),
            Box::new(TestRestFetch),
            factory,
        ));

        let user = client
            .finish_login(OgsLoginResult {
                jwt_token: "jwt".to_owned(),
                cookie_header: None,
                user: serde_json::json!({"id": 7, "username": "player"}),
            })
            .expect("the online session itself still succeeds");

        assert_eq!(user["username"], "player");
        assert_eq!(client.snapshot().user.unwrap()["id"], 7);
        assert_eq!(
            client.snapshot().last_error.as_deref(),
            Some("OGS 登录成功，但系统钥匙串不可用；重启后需要重新登录")
        );
        client.logout();
    }

    #[test]
    fn auth_error_frame_fails_login_and_marks_error() {
        let transport = Arc::new(ScriptedTransport::new(vec![Ok(Some(auth_error_reply(1)))]));
        let factory = Arc::new(ScriptedTransportFactory::new(vec![transport]));
        let client = Arc::new(LiveOgsClient::with_parts_and_transport_factory(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
            factory,
        ));
        let result = client.connect_and_authenticate("jwt");
        assert!(result.is_err());
        assert_eq!(client.snapshot().socket_status, OgsSocketStatus::Error);
    }

    #[test]
    fn reconnect_after_error_uses_a_fresh_transport() {
        let first = Arc::new(ScriptedTransport::new(vec![Ok(Some(auth_error_reply(1)))]));
        let second = Arc::new(ScriptedTransport::new(vec![Ok(Some(auth_reply(2)))]));
        let factory = Arc::new(ScriptedTransportFactory::new(vec![first, second]));
        let client = Arc::new(LiveOgsClient::with_parts_and_transport_factory(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
            factory,
        ));
        assert!(client.connect_and_authenticate("jwt").is_err());
        assert!(client.connect_and_authenticate("jwt").is_ok());
        assert_eq!(
            client.snapshot().socket_status,
            OgsSocketStatus::Authenticated
        );
        // Stop the detached reader so the test does not leak a thread.
        client.logout();
    }

    #[test]
    fn heartbeat_fires_during_continuous_incoming_traffic() {
        let transport = Arc::new(ScriptedTransport::continuous());
        let client = Arc::new(LiveOgsClient::with_parts(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
        ));
        let reader_client = Arc::clone(&client);
        let reader_transport: Arc<dyn OgsWebSocketTransport> = Arc::clone(&transport) as _;
        std::thread::spawn(move || reader_loop(reader_client, 0, reader_transport));
        std::thread::sleep(Duration::from_millis(50));
        client.stop_reader();
        assert!(
            transport
                .sent
                .lock()
                .unwrap()
                .iter()
                .any(|message| message.contains("net/ping")),
            "heartbeat must not depend on an empty receive"
        );
    }

    #[test]
    fn reader_failure_reconnects_and_reattaches_session() {
        let first = Arc::new(ScriptedTransport::new(vec![
            Ok(Some(auth_reply(1))),
            Err("connection reset".to_owned()),
        ]));
        let second = Arc::new(ScriptedTransport::new(vec![Ok(Some(auth_reply(2)))]));
        let factory = Arc::new(ScriptedTransportFactory::new(vec![
            Arc::clone(&first) as Arc<dyn OgsWebSocketTransport>,
            Arc::clone(&second) as Arc<dyn OgsWebSocketTransport>,
        ]));
        let client = Arc::new(LiveOgsClient::with_parts_and_transport_factory(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
            factory,
        ));
        client.inner.lock().unwrap().session = Some(OgsCredentials {
            server_url: "https://online-go.com".to_owned(),
            jwt_token: "jwt".to_owned(),
            cookie_header: None,
            user: Some(serde_json::json!({"id": 7})),
            created_at: None,
        });
        client
            .connect_and_authenticate("jwt")
            .expect("initial authentication succeeds");
        for _ in 0..100 {
            if client.snapshot().socket_status == OgsSocketStatus::Authenticated
                && !second.sent.lock().unwrap().is_empty()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            client.snapshot().socket_status,
            OgsSocketStatus::Authenticated
        );
        assert!(
            second
                .sent
                .lock()
                .unwrap()
                .iter()
                .any(|message| { message.contains("authenticate") })
        );
        client.logout();
    }

    fn client_with_game(user_id: u64, black_id: u64, white_id: u64) -> LiveOgsClient {
        let client = LiveOgsClient::with_parts(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
        );
        let mut inner = client.inner.lock().unwrap();
        inner.snapshot.user = Some(serde_json::json!({"id": user_id}));
        inner.snapshot.online_game = Some(OgsOnlineGame {
            game_id: 1,
            connected: true,
            black_id: Some(black_id),
            white_id: Some(white_id),
            phase: "play".to_owned(),
            next_player: Some(Color::Black),
            ..OgsOnlineGame::default()
        });
        drop(inner);
        client
    }

    #[test]
    fn participant_on_their_turn_can_play() {
        let client = client_with_game(7, 7, 8);
        assert!(client.require_my_turn(1).is_ok());
        assert!(client.require_participant(1).is_ok());
    }

    #[test]
    fn stale_cached_turn_does_not_block_participant_submission() {
        let client = client_with_game(7, 7, 8);
        client
            .inner
            .lock()
            .unwrap()
            .snapshot
            .online_game
            .as_mut()
            .unwrap()
            .next_player = Some(Color::White);
        assert!(client.require_my_turn(1).is_ok());
    }

    #[test]
    fn spectator_cannot_play_or_resign() {
        let client = client_with_game(99, 7, 8);
        assert!(client.require_my_turn(1).is_err());
        assert!(client.require_participant(1).is_err());
    }

    #[test]
    fn moves_are_rejected_outside_play_phase() {
        let client = client_with_game(7, 7, 8);
        client
            .inner
            .lock()
            .unwrap()
            .snapshot
            .online_game
            .as_mut()
            .unwrap()
            .phase = "stone removal".to_owned();
        assert!(client.require_my_turn(1).is_err());
        assert!(client.require_stone_removal(1).is_ok());
    }

    #[test]
    fn game_error_clears_phantom_game_and_sets_last_error() {
        let client = LiveOgsClient::with_parts(
            Box::new(MemoryOgsCredentialStore::available()),
            Box::new(TestRestFetch),
        );
        // Simulate the optimistic placeholder from `connect_game`.
        client.inner.lock().unwrap().snapshot.online_game = Some(OgsOnlineGame {
            game_id: 1,
            connected: false,
            ..OgsOnlineGame::default()
        });
        client.apply_game_error(1, &serde_json::json!("This is a protected game"));
        let snapshot = client.snapshot();
        assert!(snapshot.online_game.is_none());
        assert_eq!(
            snapshot.last_error.as_deref(),
            Some("This is a protected game")
        );
    }

    #[test]
    fn phase_event_updates_game_phase() {
        let client = client_with_game(7, 7, 8);
        client.apply_phase(1, &serde_json::json!("stone removal"));
        assert_eq!(
            client.snapshot().online_game.unwrap().phase,
            "stone removal"
        );
    }

    #[test]
    fn removed_stones_events_track_authoritative_state() {
        let client = client_with_game(7, 7, 8);
        client.apply_removed_stones(
            1,
            &serde_json::json!({"removed": true, "stones": "dd", "all_removed": "ddpp"}),
        );
        assert_eq!(
            client.snapshot().online_game.unwrap().removed_stones,
            "ddpp"
        );
        client.apply_removed_stones_accepted(
            1,
            &serde_json::json!({"phase": "finished", "winner": 7, "outcome": "B+2.5"}),
        );
        let game = client.snapshot().online_game.unwrap();
        assert_eq!(game.phase, "finished");
        assert_eq!(game.winner, Some(7));
        assert_eq!(game.outcome.as_deref(), Some("B+2.5"));
    }
}
