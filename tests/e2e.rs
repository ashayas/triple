use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use triple::server::{router, AppState, MediaConfig};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct Client {
    ws: Socket,
    id: u64,
}

async fn start_server() -> (String, Arc<AppState>) {
    let state = AppState::new(
        "invite-token".to_string(),
        "host-key".to_string(),
        "http://example.test/j/invite-token".to_string(),
        MediaConfig::default(),
        "test",
        "0.0.0-test",
    );
    let app = router(state.clone(), false);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    (format!("ws://{addr}"), state)
}

async fn connect(base: &str, query: &str) -> Client {
    let url = format!("{base}/ws?{query}");
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws connect");
    Client { ws, id: 0 }
}

impl Client {
    async fn send(&mut self, value: Value) {
        self.ws
            .send(Message::Text(value.to_string().into()))
            .await
            .expect("send");
    }

    async fn next(&mut self) -> Value {
        loop {
            let message = tokio::time::timeout(Duration::from_secs(3), self.ws.next())
                .await
                .expect("timed out waiting for a message")
                .expect("stream ended")
                .expect("websocket error");
            if let Message::Text(text) = message {
                return serde_json::from_str(text.as_str()).expect("json");
            }
        }
    }

    async fn next_type(&mut self, kind: &str) -> Value {
        for _ in 0..40 {
            let value = self.next().await;
            if value["type"] == kind {
                return value;
            }
        }
        panic!("never received message of type {kind}");
    }

    async fn hello(&mut self, name: &str) -> Value {
        self.send(json!({"type": "hello", "name": name})).await;
        let welcome = self.next_type("welcome").await;
        self.id = welcome["self"]["id"].as_u64().expect("id");
        welcome
    }

    async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

#[tokio::test]
async fn host_and_guest_join_and_chat() {
    let (base, _state) = start_server().await;

    let mut host = connect(&base, "key=host-key").await;
    let welcome = host.hello("Host").await;
    assert_eq!(welcome["self"]["role"], "host");
    assert_eq!(welcome["participants"].as_array().unwrap().len(), 1);
    assert!(welcome["invite_url"]
        .as_str()
        .unwrap()
        .contains("/j/invite-token"));

    let mut guest = connect(&base, "token=invite-token").await;
    let guest_welcome = guest.hello("Ada").await;
    assert_eq!(guest_welcome["self"]["role"], "guest");
    assert_eq!(guest_welcome["participants"].as_array().unwrap().len(), 2);
    assert!(guest_welcome["invite_url"].is_null());

    let joined = host.next_type("join").await;
    assert_eq!(joined["participant"]["name"], "Ada");

    host.send(json!({"type": "chat", "text": "hello Ada"}))
        .await;
    let chat = guest.next_type("chat").await;
    assert_eq!(chat["text"], "hello Ada");
    assert_eq!(chat["name"], "Host");

    guest.send(json!({"type": "reaction", "emoji": "🎉"})).await;
    let reaction = host.next_type("reaction").await;
    assert_eq!(reaction["emoji"], "🎉");
    assert_eq!(reaction["name"], "Ada");

    guest.close().await;
    let left = host.next_type("leave").await;
    assert!(left["id"].is_number());
}

#[tokio::test]
async fn code_editing_requires_consent_then_converges() {
    let (base, _state) = start_server().await;

    let mut host = connect(&base, "key=host-key").await;
    host.hello("Host").await;
    let mut guest = connect(&base, "token=invite-token").await;
    guest.hello("Ada").await;
    host.next_type("join").await;

    guest
        .send(json!({
            "type": "ot",
            "rev": 0,
            "ops": [{"insert": "blocked"}]
        }))
        .await;
    let denied = guest.next_type("error").await;
    assert!(denied["message"].as_str().unwrap().contains("view-only"));
    let resync = guest.next_type("ot_resync").await;
    assert_eq!(resync["text"], "");
    assert_eq!(resync["rev"], 0);

    host.send(json!({"type": "control_response", "to": guest.id, "granted": true}))
        .await;
    let state = guest.next_type("control_state").await;
    assert_eq!(state["granted"], true);
    guest.next_type("control_result").await;

    guest
        .send(json!({"type": "ot", "rev": 0, "ops": [{"insert": "hello"}]}))
        .await;
    let ack = guest.next_type("ot_ack").await;
    assert_eq!(ack["rev"], 1);
    let relayed = host.next_type("ot").await;
    assert_eq!(relayed["rev"], 1);

    host.send(json!({"type": "resync"})).await;
    let snapshot = host.next_type("ot_resync").await;
    assert_eq!(snapshot["text"], "hello");
    assert_eq!(snapshot["rev"], 1);

    guest
        .send(json!({
            "type": "ot",
            "rev": 1,
            "ops": [{"retain": 5}, {"insert": " world"}]
        }))
        .await;
    assert_eq!(guest.next_type("ot_ack").await["rev"], 2);
    host.next_type("ot").await;

    // Concurrent edits from both sides at revision 2.
    host.send(json!({
        "type": "ot",
        "rev": 2,
        "ops": [{"retain": 11}, {"insert": "!"}]
    }))
    .await;
    guest
        .send(json!({
            "type": "ot",
            "rev": 2,
            "ops": [{"retain": 11}, {"insert": "?"}]
        }))
        .await;
    guest.next_type("ot_ack").await;
    host.next_type("ot_ack").await;
    // The host also receives the guest's transformed concurrent op.
    host.next_type("ot").await;

    let mut late = connect(&base, "token=invite-token").await;
    let welcome = late.hello("Late").await;
    let text = welcome["doc"]["text"].as_str().unwrap();
    assert!(text.starts_with("hello world"), "{text}");
    assert!(text.contains('!'), "{text}");
    assert!(text.contains('?'), "{text}");
}

#[tokio::test]
async fn everyone_can_edit_policy_applies_to_new_ops() {
    let (base, _state) = start_server().await;
    let mut host = connect(&base, "key=host-key").await;
    host.hello("Host").await;
    let mut guest = connect(&base, "token=invite-token").await;
    guest.hello("Ada").await;
    host.next_type("join").await;

    host.send(json!({"type": "edit_policy", "everyone": true}))
        .await;
    let policy = guest.next_type("edit_policy").await;
    assert_eq!(policy["everyone"], true);

    guest
        .send(json!({"type": "ot", "rev": 0, "ops": [{"insert": "shared"}]}))
        .await;
    let ack = guest.next_type("ot_ack").await;
    assert_eq!(ack["rev"], 1);
}

#[tokio::test]
async fn presenter_and_signalling_relay() {
    let (base, _state) = start_server().await;
    let mut host = connect(&base, "key=host-key").await;
    host.hello("Host").await;
    let mut guest = connect(&base, "token=invite-token").await;
    guest.hello("Ada").await;
    let joined = host.next_type("join").await;
    let guest_id = joined["participant"]["id"].as_u64().unwrap();

    host.send(json!({"type": "set_presenter", "target": guest_id}))
        .await;
    let presenter = guest.next_type("presenter").await;
    assert_eq!(presenter["id"], guest_id);
    host.next_type("presenter").await;

    guest
        .send(json!({
            "type": "signal",
            "to": host.id,
            "data": {"kind": "description", "type": "offer", "sdp": "v=0"}
        }))
        .await;
    let signal = host.next_type("signal").await;
    assert_eq!(signal["from"], guest_id);
    assert_eq!(signal["data"]["sdp"], "v=0");

    host.send(json!({"type": "set_presenter", "target": null}))
        .await;
    assert!(guest.next_type("presenter").await["id"].is_null());
}

#[tokio::test]
async fn bad_invite_token_is_rejected() {
    let (base, _state) = start_server().await;
    let url = format!("{base}/ws?token=nope");
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
}

#[tokio::test]
async fn host_key_cannot_be_used_as_guest_token() {
    let (base, _state) = start_server().await;
    let url = format!("{base}/ws?token=host-key");
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
}
