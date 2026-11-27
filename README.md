<div align="center">

# triple

### Screen sharing for people who build software.

Because better discussion leads to faster production — and a shared buffer beats a status meeting.

[![License: MIT](https://img.shields.io/badge/license-MIT-7c9cff?style=flat-square)](LICENSE)
[![Rust 1.84+](https://img.shields.io/badge/rust-1.84%2B-b7410e?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-13%20passing-47d18a?style=flat-square)](tests)
[![No login](https://img.shields.io/badge/login-none-8d97ab?style=flat-square)](#no-account-no-install)

[![triple in action: shared screen with annotation, a synced scratchpad, presence and reactions](docs/screenshot.png)](https://github.com/ashayas/triple)

**[Get triple](https://github.com/ashayas/triple)** · [How it works](#how-it-works) · [What it doesn't do](#what-it-doesnt-do) · [Docs](#documentation)

</div>

## Start in two commands

```sh
cargo run --release
```

The banner prints a **host view** for your browser and an **invite link** for
your pair. Send the link; they open it in a browser. That is the whole setup — no
account, no install for the guest, no cloud dependency for the host.

> Host view opens over loopback, so you never see the certificate prompt. Guests
> accept a self-signed cert once on a LAN, or get a trusted cert from your proxy
> or tunnel. Prefer to look first? The screenshot above is the entire UI: stage,
> scratchpad, people, annotation, reactions.

## Built for working, not for meetings

Generic screen sharing is built for all-hands calls. triple is built for the loop
where two people read the same stack trace, argue about a name, and ship the fix.
No calendar, no dial-in, no "you're on mute".

## One click to swap who's sharing

Presenter is a single piece of session state. The host can hand the stage to a
guest from the People tab, a guest can ask for it with **Request to present**, and
whoever holds it shares their screen. Everyone else sees "Presented by …" and the
swap happens without reconnecting, renegotiating everything, or starting a new
call.

## Seamless collaboration

- **Annotate the shared screen** — freehand strokes in your colour, cleared by
  anyone, replayed to late joiners.
- **Point at things** — live cursors with names, carried over the data channel.
- **Share the buffer** — a scratchpad with server-authoritative operational
  transform: concurrent edits converge, late joiners get the current text and
  revision, and the host decides whether everyone can edit or only granted guests.
- **Stay in context** — chat and reactions are part of the session, and late
  joiners can still read them.

## Ridiculously crisp audio and video

Screen share runs at the resolution your display and bandwidth allow, through
WebRTC, with system audio when the browser exposes it. Camera and mic are
separate toggles with their own mute/stop. There is no transcoding, no
server-side re-encode, and no relay unless you configure one.

## Peer-to-peer by default

Media travels browser-to-browser over DTLS-SRTP and never transits the Rust
process. That is encrypted peer-to-peer transport — not a hosted media relay.
The server handles signalling, presence, chat, annotation and document
operations. When a direct path is impossible (symmetric NAT, strict firewall),
point triple at your own TURN server; the server still never sees the media.

## No account, no install

The invite link **is** the capability: a 128-bit random token, plus a separate
host key that cannot be used as a guest invite. Run it:

- on the public internet with `--tunnel` (cloudflared quick tunnel),
- on Wi-Fi/LAN with the default self-signed TLS,
- or behind a whitelisted internal domain with `--public-url`.

Guests need a browser with WebRTC. Nothing to install, nothing to sign up for.

## Pick exactly what you share

`getDisplayMedia` gives you the browser's native picker, so you choose the source
yourself: entire screen, a single window, or a tab — with system audio where the
platform allows it. Camera and mic are independent toggles. Stop sharing at any
time; nothing keeps recording.

## Consent-gated control

Control is not remote desktop. The host grants it per guest, and it only unlocks
the **shared scratchpad and annotation layer**. There is no shell, no OS input
injection, no file browsing, no screen-scraping channel, and no unattended
access. Revoke it and the guest is view-only again.

## How it works

1. **Run the binary.** `cargo run --release`
2. **Send the unguessable link** from the banner.
3. **Pair.** Share a screen, annotate, edit the scratchpad together, hand off the
   stage.

## What's in the box

| | Capability |
| :--: | --- |
| ✅ | One-command host, unguessable invite link, no signup |
| ✅ | Screen share at source resolution, with system audio |
| ✅ | Camera + microphone with independent mute/stop |
| ✅ | Presenter hand-off and request-to-present |
| ✅ | Multi-guest sessions (up to 16) with presence |
| ✅ | Shared scratchpad with real OT, late-join sync and live edit status |
| ✅ | Host-only editing by default, "everyone can edit" toggle |
| ✅ | Per-guest control grants, revocable at any time |
| ✅ | Freehand annotation over the shared screen |
| ✅ | Remote pointers with names |
| ✅ | Chat and emoji reactions, replayed to late joiners |
| ✅ | Clipboard sharing on an explicit gesture — never automatic |
| ✅ | Self-signed TLS for LAN, reverse-proxy/internal-domain mode, optional tunnel |
| ✅ | STUN + configurable TURN for restrictive networks |
| ✅ | Media never transits the Rust process |

## What it doesn't do

- ❌ **No recording or replay.** State lives only while the host process runs.
- ❌ **No desktop, shell, or OS input control.** Control is limited to the
  scratchpad and annotation layer, by design.
- ❌ **No persistence.** No accounts, rooms, history, or audit log.
- ❌ **No syntax highlighting, LSP, or IDE integration.** The scratchpad is plain
  text with real convergence.
- ❌ **No teams or billing.** The link is the capability.
- ❌ **No native app.** Any modern browser with WebRTC works.
- ❌ **No App Veil, triggers, or AI features.** See the parity notes below.

---

# Documentation

## Modes

| Mode | Command | TLS |
| --- | --- | --- |
| LAN / local (default) | `triple` | Self-signed cert generated once and cached |
| Reverse proxy / internal domain | `triple --public-url https://triple.corp.example` (or `--domain triple.corp.example`) | Terminated by your proxy or load balancer |
| Public tunnel, no port forwarding | `triple --tunnel` | cloudflared quick tunnel (trusted HTTPS) |
| Plain HTTP (development only) | `triple --http` | None — remote guests lose the secure context needed for capture |

`--tunnel` requires `cloudflared` on `PATH`; if it is missing, triple falls back
to LAN mode and says so.

## Configuration

Flags also read environment variables.

| Flag | Env | Default |
| --- | --- | --- |
| `--bind` | `TRIPLE_BIND` | `0.0.0.0` |
| `--port` | `TRIPLE_PORT` | `7800` |
| `--host-port` | `TRIPLE_HOST_PORT` | `7801` |
| `--public-url` | `TRIPLE_PUBLIC_URL` | — |
| `--domain` | `TRIPLE_DOMAIN` | — |
| `--tunnel` | `TRIPLE_TUNNEL` | off |
| `--http` | `TRIPLE_HTTP` | off |
| `--cert-dir` | `TRIPLE_CERT_DIR` | OS config dir |
| `--stun` | `TRIPLE_STUN` | `stun:stun.l.google.com:19302` |
| `--turn-url` | `TRIPLE_TURN_URL` | — |
| `--turn-user` | `TRIPLE_TURN_USER` | — |
| `--turn-pass` | `TRIPLE_TURN_PASS` | — |
| `--advertise-ip` | `TRIPLE_ADVERTISE_IP` | primary interface |

Example with a TURN relay for locked-down corporate networks:

```sh
TRIPLE_PUBLIC_URL=https://triple.corp.example \
TRIPLE_TURN_URL=turns:turn.corp.example:5349 \
TRIPLE_TURN_USER=triple TRIPLE_TURN_PASS=... \
triple
```

## Deployment

### Reverse proxy / internal domain

Point DNS at the machine (or its load balancer), let the proxy terminate TLS and
forward WebSockets, then run triple with `--public-url`. Invite links and the host
view use the public URL, and the secure context comes from the proxy's TLS, so
screen capture works.

Caddy:

```caddyfile
triple.corp.example {
    reverse_proxy 127.0.0.1:7800
}
```

nginx:

```nginx
location / {
    proxy_pass http://127.0.0.1:7800;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

The process serves plain HTTP on `--bind/--port` in this mode; do not expose that
port directly.

### TURN

Signalling, presence, chat, annotations and OT go over the WebSocket. Screen,
camera and mic are WebRTC peer-to-peer. When both peers are behind symmetric NAT
or a restrictive firewall, configure TURN (`TRIPLE_TURN_*`); the Rust process
still never carries media.

## Security model

- 128-bit random invite token and a separate host key; the host key is rejected
  as a guest invite.
- No shell, no OS input injection, no discovery scanning, no unattended access.
- Control is limited to the scratchpad and annotation layer, granted per guest
  and revocable at any time.
- Media is encrypted between browsers (DTLS-SRTP); the server sees signalling,
  presence, chat, strokes and document operations, and persists nothing to disk.
- CSP is `script-src 'self'`; the client ships as static assets from the binary.

## Parity with Tuple

Implemented here: link-based pairing, screen share with system audio, cam/mic,
presenter swap, annotation, pointers, OT scratchpad, chat, reactions, clipboard
on a gesture, LAN/proxy/tunnel hosting, TURN.

Deliberately or currently absent: App Veil (hiding apps/notifications), native
macOS/Windows apps, keyboard-shortcut suite, Triggers/API, recording, teams and
billing, audit logs, persistent rooms, OS-level remote control, and automatic
clipboard sync.

## Tests

```sh
cargo test
```

Covers OT transform/apply/diff convergence, tunnel URL parsing, and end-to-end
WebSocket sessions: join, chat, edit policy, consent plus concurrent-edit
convergence, presenter/signalling relay, and invite rejection.

Browser UI verification is manual (or Playwright-driven): join as host and guest,
share a screen, annotate, toggle edit policy, request/grant control, send a
reaction and clipboard. On macOS, the browser needs Screen Recording permission
for `getDisplayMedia` to deliver frames; without it the track negotiates but
carries no frames. Camera/mic use the normal per-site permission prompt.

## License

MIT. See [LICENSE](LICENSE).

<sub>triple · MIT · [GitHub](https://github.com/ashayas/triple) · not affiliated with Tuple</sub>
