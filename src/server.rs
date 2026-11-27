use crate::session::{ClientMsg, Role, Session, MAX_PARTICIPANTS};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaConfig {
    pub ice_servers: Vec<IceServer>,
}

impl MediaConfig {
    pub fn is_relay_configured(&self) -> bool {
        self.ice_servers
            .iter()
            .any(|s| s.urls.iter().any(|u| u.starts_with("turn:")))
    }
}

pub struct AppState {
    pub session: Mutex<Session>,
    pub token: String,
    pub host_key: String,
    pub media: MediaConfig,
    pub mode: String,
    pub version: String,
}

impl AppState {
    pub fn new(
        token: String,
        host_key: String,
        invite_url: String,
        media: MediaConfig,
        mode: impl Into<String>,
        version: impl Into<String>,
    ) -> Arc<Self> {
        let version = version.into();
        Arc::new(Self {
            session: Mutex::new(Session::new(invite_url, version.clone())),
            token,
            host_key,
            media,
            mode: mode.into(),
            version,
        })
    }
}

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

pub fn router(state: Arc<AppState>, host_listener: bool) -> Router {
    let key = state.host_key.clone();
    let mode = state.mode.clone();
    let index_route = get(move || {
        let key = key.clone();
        let mode = mode.clone();
        async move {
            if host_listener {
                Redirect::temporary(&format!("/h/{key}")).into_response()
            } else {
                (csp_headers(), Html(landing(&mode))).into_response()
            }
        }
    });

    Router::new()
        .route("/", index_route)
        .route("/h/{key}", get(host_page))
        .route("/j/{token}", get(guest_page))
        .route("/assets/app.js", get(asset_js))
        .route("/assets/style.css", get(asset_css))
        .route("/api/config", get(api_config))
        .route("/healthz", get(|| async { "ok" }))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

fn csp_headers() -> [(header::HeaderName, &'static str); 3] {
    [
        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
        (
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self' ws: wss:; font-src 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    ]
}

async fn asset_js() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        APP_JS,
    )
        .into_response()
}

async fn asset_css() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        STYLE_CSS,
    )
        .into_response()
}

async fn host_page(Path(key): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    if key == state.host_key {
        (csp_headers(), Html(INDEX_HTML)).into_response()
    } else {
        not_found_page()
    }
}

async fn guest_page(Path(token): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    if token == state.token {
        (csp_headers(), Html(INDEX_HTML)).into_response()
    } else {
        not_found_page()
    }
}

async fn api_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "iceServers": state.media.ice_servers,
        "mode": state.mode,
        "version": state.version,
    }))
}

fn not_found_page() -> Response {
    (
        StatusCode::NOT_FOUND,
        (csp_headers(), Html(
            "<!doctype html><meta charset=\"utf-8\"><title>triple</title>\
             <body style=\"font:16px/1.5 system-ui;background:#0b0d12;color:#e8ecf4;display:grid;place-items:center;height:100vh;margin:0\">\
             <div style=\"text-align:center\"><h1 style=\"font-weight:600\">Link not valid</h1>\
             <p style=\"color:#9aa4b8\">This invite link is wrong or the session has ended.<br>Ask the host for a fresh link.</p></div></body>"
        )),
    )
        .into_response()
}

fn landing(mode: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>triple</title>\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <body style=\"font:16px/1.5 system-ui;background:#0b0d12;color:#e8ecf4;display:grid;place-items:center;height:100vh;margin:0\">\
         <div style=\"text-align:center;max-width:36rem;padding:2rem\">\
         <div style=\"font-size:2rem;font-weight:700;letter-spacing:-.02em\">&#9651; triple</div>\
         <h1 style=\"font-weight:600;margin:1.5rem 0 .5rem\">Open the invite link</h1>\
         <p style=\"color:#9aa4b8;margin:0\">Triple sessions are reachable only through the unguessable link the host shared. \
         If you are the host, open the <em>host view</em> URL printed in your terminal.</p>\
         <p style=\"color:#5b6478;font-size:.8rem;margin-top:2rem\">mode: {mode}</p>\
         </div></body>"
    )
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let role = if params
        .get("key")
        .map(|k| k == &state.host_key)
        .unwrap_or(false)
    {
        Role::Host
    } else if params
        .get("token")
        .map(|t| t == &state.token)
        .unwrap_or(false)
    {
        Role::Guest
    } else {
        return (StatusCode::UNAUTHORIZED, "invalid invite").into_response();
    };

    let full = {
        let session = state.session.lock().unwrap();
        session.peer_count() >= MAX_PARTICIPANTS
    };
    if full {
        return (StatusCode::SERVICE_UNAVAILABLE, "session is full").into_response();
    }

    ws.max_message_size(1024 * 1024)
        .max_frame_size(1024 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, state, role))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, role: Role) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let id = {
        let mut session = state.session.lock().unwrap();
        session.connect(role, tx.clone())
    };

    let send_task = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = stream.next().await {
        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMsg>(text.as_str()) {
                Ok(msg) => {
                    let mut session = state.session.lock().unwrap();
                    session.handle(id, msg);
                }
                Err(_) => {
                    let _ = tx.send(
                        json!({"type": "error", "message": "Unsupported message"}).to_string(),
                    );
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.session.lock().unwrap().leave(id);
    send_task.abort();
}
