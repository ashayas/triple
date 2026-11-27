//! Session state: presence, presenter, chat, annotations, consent and the
//! shared document. All mutation happens under one lock, which also gives the
//! code editor a single global ordering.

use crate::ot::{ApplyError, Document, Op};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

pub const MAX_NAME: usize = 40;
pub const MAX_CHAT: usize = 4_000;
pub const MAX_CLIPBOARD: usize = 64 * 1024;
pub const MAX_STROKE_POINTS: usize = 4_000;
pub const MAX_PARTICIPANTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Guest,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Host => "host",
            Role::Guest => "guest",
        }
    }
}

pub struct Peer {
    pub id: u64,
    pub name: String,
    pub role: Role,
    pub tx: UnboundedSender<String>,
    pub control: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stroke {
    pub id: u64,
    pub from: u64,
    pub name: String,
    pub color: String,
    pub width: f64,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        name: String,
    },
    Chat {
        text: String,
    },
    Reaction {
        emoji: String,
    },
    Annotation {
        color: String,
        width: f64,
        points: Vec<[f64; 2]>,
    },
    AnnotationClear,
    Clipboard {
        text: String,
    },
    Signal {
        to: u64,
        data: Value,
    },
    SetPresenter {
        target: Option<u64>,
    },
    PresentRequest,
    ControlRequest,
    ControlResponse {
        to: u64,
        granted: bool,
    },
    ControlRevoke {
        target: u64,
    },
    EditPolicy {
        everyone: bool,
    },
    Ot {
        rev: u64,
        ops: Vec<Op>,
    },
    Resync,
    Pointer {
        x: f64,
        y: f64,
        active: bool,
    },
    Ping,
}

pub struct Session {
    peers: HashMap<u64, Peer>,
    next_id: u64,
    next_stroke_id: u64,
    doc: Document,
    presenter: Option<u64>,
    policy_everyone: bool,
    chat: VecDeque<Value>,
    strokes: VecDeque<Stroke>,
    invite_url: String,
    version: String,
}

impl Session {
    pub fn new(invite_url: String, version: impl Into<String>) -> Self {
        Self {
            peers: HashMap::new(),
            next_id: 1,
            next_stroke_id: 1,
            doc: Document::new(""),
            presenter: None,
            policy_everyone: false,
            chat: VecDeque::new(),
            strokes: VecDeque::new(),
            invite_url,
            version: version.into(),
        }
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn connect(&mut self, role: Role, tx: UnboundedSender<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.peers.insert(
            id,
            Peer {
                id,
                name: if role == Role::Host {
                    "Host".into()
                } else {
                    "Guest".into()
                },
                role,
                tx,
                control: false,
            },
        );
        id
    }

    fn send_to(&self, id: u64, msg: Value) {
        if let Some(peer) = self.peers.get(&id) {
            let _ = peer.tx.send(msg.to_string());
        }
    }

    fn broadcast(&self, msg: Value, except: Option<u64>) {
        let text = msg.to_string();
        for peer in self.peers.values() {
            if Some(peer.id) == except {
                continue;
            }
            let _ = peer.tx.send(text.clone());
        }
    }

    fn host_ids(&self) -> Vec<u64> {
        self.peers
            .values()
            .filter(|p| p.role == Role::Host)
            .map(|p| p.id)
            .collect()
    }

    fn can_edit(&self, peer: &Peer) -> bool {
        peer.role == Role::Host || self.policy_everyone || peer.control
    }

    fn peer_json(&self, peer: &Peer) -> Value {
        json!({
            "id": peer.id,
            "name": peer.name,
            "role": peer.role.as_str(),
            "presenting": self.presenter == Some(peer.id),
            "can_edit": self.can_edit(peer),
            "has_control": peer.control,
        })
    }

    fn participants(&self) -> Vec<Value> {
        let mut peers: Vec<&Peer> = self.peers.values().collect();
        peers.sort_by_key(|p| p.id);
        peers.iter().map(|p| self.peer_json(p)).collect()
    }

    pub fn handle(&mut self, id: u64, msg: ClientMsg) {
        let Some(peer) = self.peers.get(&id) else {
            return;
        };
        let role = peer.role;
        match msg {
            ClientMsg::Hello { name } => {
                let name = sanitize_name(&name);
                if let Some(peer) = self.peers.get_mut(&id) {
                    peer.name = name.clone();
                }
                let participant = self
                    .peers
                    .get(&id)
                    .map(|p| self.peer_json(p))
                    .unwrap_or(Value::Null);
                self.broadcast(
                    json!({"type": "join", "participant": participant}),
                    Some(id),
                );

                let me = self
                    .peers
                    .get(&id)
                    .map(|p| self.peer_json(p))
                    .unwrap_or(Value::Null);
                let can_edit = self
                    .peers
                    .get(&id)
                    .map(|p| self.can_edit(p))
                    .unwrap_or(false);
                let welcome = json!({
                    "type": "welcome",
                    "self": {"id": id, "name": name, "role": role.as_str()},
                    "participants": self.participants(),
                    "presenter": self.presenter,
                    "policy_everyone": self.policy_everyone,
                    "can_edit": can_edit,
                    "doc": {"text": self.doc.text(), "rev": self.doc.rev()},
                    "chat": self.chat.iter().cloned().collect::<Vec<_>>(),
                    "strokes": self.strokes.iter().cloned().collect::<Vec<_>>(),
                    "invite_url": if role == Role::Host { Value::String(self.invite_url.clone()) } else { Value::Null },
                    "version": self.version,
                    "you": me,
                });
                self.send_to(id, welcome);
            }
            ClientMsg::Chat { text } => {
                let text = text.trim();
                if text.is_empty() {
                    return;
                }
                let text: String = text.chars().take(MAX_CHAT).collect();
                let entry = json!({
                    "type": "chat",
                    "from": id,
                    "name": peer.name,
                    "text": text,
                    "ts": now_ms(),
                });
                self.chat.push_back(entry.clone());
                while self.chat.len() > 100 {
                    self.chat.pop_front();
                }
                self.broadcast(entry, None);
            }
            ClientMsg::Reaction { emoji } => {
                let emoji: String = emoji.chars().take(8).collect();
                if emoji.is_empty() {
                    return;
                }
                self.broadcast(
                    json!({"type": "reaction", "from": id, "name": peer.name, "emoji": emoji}),
                    None,
                );
            }
            ClientMsg::Annotation {
                color,
                width,
                points,
            } => {
                if points.is_empty() {
                    return;
                }
                let color = sanitize_color(&color);
                let width = width.clamp(0.5, 24.0);
                let points: Vec<[f64; 2]> = points
                    .into_iter()
                    .take(MAX_STROKE_POINTS)
                    .map(|[x, y]| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])
                    .collect();
                let stroke = Stroke {
                    id: self.next_stroke_id,
                    from: id,
                    name: peer.name.clone(),
                    color,
                    width,
                    points,
                };
                self.next_stroke_id += 1;
                self.strokes.push_back(stroke.clone());
                while self.strokes.len() > 300 {
                    self.strokes.pop_front();
                }
                self.broadcast(json!({"type": "annotation", "stroke": stroke}), None);
            }
            ClientMsg::AnnotationClear => {
                self.strokes.clear();
                self.broadcast(json!({"type": "annotation_clear"}), None);
            }
            ClientMsg::Clipboard { text } => {
                let text: String = text.chars().take(MAX_CLIPBOARD).collect();
                if text.is_empty() {
                    return;
                }
                self.broadcast(
                    json!({"type": "clipboard", "from": id, "name": peer.name, "text": text}),
                    Some(id),
                );
            }
            ClientMsg::Signal { to, data } => {
                if self.peers.contains_key(&to) {
                    self.send_to(to, json!({"type": "signal", "from": id, "data": data}));
                }
            }
            ClientMsg::SetPresenter { target } => {
                if role != Role::Host {
                    self.send_to(id, error("Only the host can change the presenter"));
                    return;
                }
                if let Some(target) = target {
                    if !self.peers.contains_key(&target) {
                        return;
                    }
                }
                self.presenter = target;
                self.broadcast(json!({"type": "presenter", "id": target}), None);
            }
            ClientMsg::PresentRequest => {
                for host in self.host_ids() {
                    self.send_to(
                        host,
                        json!({"type": "present_request", "from": id, "name": peer.name}),
                    );
                }
            }
            ClientMsg::ControlRequest => {
                for host in self.host_ids() {
                    if host == id {
                        continue;
                    }
                    self.send_to(
                        host,
                        json!({"type": "control_request", "from": id, "name": peer.name}),
                    );
                }
            }
            ClientMsg::ControlResponse { to, granted } => {
                if role != Role::Host {
                    return;
                }
                if let Some(target) = self.peers.get_mut(&to) {
                    target.control = granted;
                }
                self.broadcast(
                    json!({"type": "control_state", "id": to, "granted": granted}),
                    None,
                );
                self.send_to(to, json!({"type": "control_result", "granted": granted}));
            }
            ClientMsg::ControlRevoke { target } => {
                if role != Role::Host {
                    return;
                }
                if let Some(target) = self.peers.get_mut(&target) {
                    target.control = false;
                }
                self.broadcast(
                    json!({"type": "control_state", "id": target, "granted": false}),
                    None,
                );
            }
            ClientMsg::EditPolicy { everyone } => {
                if role != Role::Host {
                    return;
                }
                self.policy_everyone = everyone;
                self.broadcast(json!({"type": "edit_policy", "everyone": everyone}), None);
            }
            ClientMsg::Ot { rev, ops } => {
                let can_edit = self
                    .peers
                    .get(&id)
                    .map(|p| self.can_edit(p))
                    .unwrap_or(false);
                if !can_edit {
                    self.send_to(
                        id,
                        error("You have view-only access. Ask the host for control."),
                    );
                    self.send_doc(id);
                    return;
                }
                match self.doc.apply(rev, ops) {
                    Ok((new_rev, transformed)) => {
                        self.send_to(id, json!({"type": "ot_ack", "rev": new_rev}));
                        self.broadcast(
                            json!({"type": "ot", "rev": new_rev, "ops": transformed, "from": id}),
                            Some(id),
                        );
                    }
                    Err(ApplyError::Stale) | Err(ApplyError::Future) | Err(ApplyError::Invalid) => {
                        self.send_doc(id);
                    }
                }
            }
            ClientMsg::Resync => self.send_doc(id),
            ClientMsg::Pointer { x, y, active } => {
                self.broadcast(
                    json!({
                        "type": "pointer",
                        "from": id,
                        "name": peer.name,
                        "x": x.clamp(0.0, 1.0),
                        "y": y.clamp(0.0, 1.0),
                        "active": active,
                    }),
                    Some(id),
                );
            }
            ClientMsg::Ping => self.send_to(id, json!({"type": "pong"})),
        }
    }

    fn send_doc(&self, id: u64) {
        self.send_to(
            id,
            json!({
                "type": "ot_resync",
                "text": self.doc.text(),
                "rev": self.doc.rev(),
            }),
        );
    }

    pub fn leave(&mut self, id: u64) {
        if self.peers.remove(&id).is_none() {
            return;
        }
        self.broadcast(json!({"type": "leave", "id": id}), None);
        if self.presenter == Some(id) {
            self.presenter = None;
            self.broadcast(json!({"type": "presenter", "id": null}), None);
        }
    }
}

fn error(message: &str) -> Value {
    json!({"type": "error", "message": message})
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "Guest".to_string()
    } else {
        cleaned
    }
}

fn sanitize_color(color: &str) -> String {
    let valid = color.len() == 7
        && color.starts_with('#')
        && color.chars().skip(1).all(|c| c.is_ascii_hexdigit());
    if valid {
        color.to_string()
    } else {
        "#ff5c5c".to_string()
    }
}
