# triple

Tuple-style pair programming, built in Rust. Run one binary, hand out one
unguessable link, and pair with anyone — over the public internet, on a Wi-Fi /
LAN, or behind a whitelisted company domain. No accounts, no login, no cloud
dependency for the host.

> Work in progress. Feature parity and limitations are documented as they land.

## Deployment modes

| Mode | How | TLS |
| --- | --- | --- |
| Local / LAN (fallback) | `triple` | Self-signed cert generated on first run; one-time browser trust prompt for guests |
| Behind a reverse proxy / internal domain | `triple --public-url https://triple.corp.example` | Terminated by your proxy or load balancer |
| Public tunnel (no port forwarding) | `triple --tunnel` | `cloudflared` quick tunnel (HTTPS, trusted cert) |

Media (screen, camera, mic) is peer-to-peer over WebRTC with configurable STUN
and TURN servers; the Rust process is the signalling and presence server and
never relays media.

## License

MIT. See [LICENSE](LICENSE).
