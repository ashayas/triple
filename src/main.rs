use anyhow::{bail, Context, Result};
use clap::Parser;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use triple::{
    certs, net,
    server::{self, AppState, IceServer, MediaConfig},
    tunnel,
};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "triple",
    version,
    about = "Tuple-style pair programming over the internet, LAN or your internal domain.",
    long_about = None
)]
struct Args {
    /// Address for the main listener (guests, and the proxy in --public-url mode).
    #[arg(long, default_value = "0.0.0.0", env = "TRIPLE_BIND")]
    bind: IpAddr,

    /// Main listener port. Guests connect here (HTTPS in local mode, HTTP behind a proxy/tunnel).
    #[arg(long, default_value_t = 7800, env = "TRIPLE_PORT")]
    port: u16,

    /// Loopback HTTP port for the host's own browser in local TLS mode (localhost is a
    /// secure context, so the host never sees a certificate warning).
    #[arg(long, default_value_t = 7801, env = "TRIPLE_HOST_PORT")]
    host_port: u16,

    /// Public base URL when running behind a reverse proxy or internal load balancer,
    /// e.g. https://triple.corp.example. TLS is terminated upstream and this process
    /// serves plain HTTP on --bind/--port.
    #[arg(long, env = "TRIPLE_PUBLIC_URL")]
    public_url: Option<String>,

    /// Shorthand for --public-url https://<domain>.
    #[arg(long, env = "TRIPLE_DOMAIN")]
    domain: Option<String>,

    /// Start a cloudflared quick tunnel and advertise its HTTPS URL. Requires cloudflared
    /// on PATH; falls back to LAN mode if it is missing or fails.
    #[arg(long, env = "TRIPLE_TUNNEL")]
    tunnel: bool,

    /// Serve plain HTTP on the main listener (development only; remote guests lose the
    /// secure context required for screen capture and getUserMedia).
    #[arg(long, env = "TRIPLE_HTTP")]
    http: bool,

    /// Directory for the generated self-signed certificate.
    #[arg(long, env = "TRIPLE_CERT_DIR")]
    cert_dir: Option<PathBuf>,

    /// STUN servers, comma separated.
    #[arg(
        long,
        env = "TRIPLE_STUN",
        default_value = "stun:stun.l.google.com:19302"
    )]
    stun: String,

    /// TURN relay URL, e.g. turn:turn.example.com:3478 (or turns:...:5349).
    #[arg(long, env = "TRIPLE_TURN_URL")]
    turn_url: Option<String>,

    /// TURN username.
    #[arg(long, env = "TRIPLE_TURN_USER")]
    turn_user: Option<String>,

    /// TURN credential.
    #[arg(long, env = "TRIPLE_TURN_PASS")]
    turn_pass: Option<String>,

    /// IP to advertise in the LAN invite link (defaults to the primary interface).
    #[arg(long, env = "TRIPLE_ADVERTISE_IP")]
    advertise_ip: Option<IpAddr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Proxy,
    Tunnel,
    LocalTls,
    PlainHttp,
}

impl Mode {
    fn slug(self) -> &'static str {
        match self {
            Mode::Proxy => "proxy",
            Mode::Tunnel => "tunnel",
            Mode::LocalTls => "local-tls",
            Mode::PlainHttp => "plain-http",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Mode::Proxy => "behind a reverse proxy / internal domain (TLS terminated upstream)",
            Mode::Tunnel => "public cloudflared quick tunnel",
            Mode::LocalTls => "LAN / local (self-signed TLS)",
            Mode::PlainHttp => "plain HTTP (development only)",
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let args = Args::parse();
    if let Err(error) = run(args).await {
        eprintln!("\ntriple: {error:#}");
        std::process::exit(1);
    }
}

async fn run(args: Args) -> Result<()> {
    if args.host_port == args.port {
        bail!("--host-port must differ from --port (both would bind the same port)");
    }

    let version = env!("CARGO_PKG_VERSION").to_string();
    let token = random_hex(16);
    let host_key = random_hex(16);
    let media = build_media(&args);
    let lan_ips = net::all_lan_ips();
    let preferred_ip = args
        .advertise_ip
        .or_else(net::preferred_ip)
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let lan_host = net::url_host(preferred_ip);

    let explicit_base = args.public_url.as_deref().map(normalize_base).or_else(|| {
        args.domain
            .as_deref()
            .map(|d| format!("https://{}", d.trim().trim_end_matches('/')))
    });

    let mut tunnel_guard: Option<tunnel::Tunnel> = None;
    let (mode, public_base) = if let Some(base) = explicit_base {
        (Mode::Proxy, base)
    } else if args.tunnel {
        match tunnel::start_quick_tunnel(args.port).await {
            Ok(t) => {
                let url = t.url.clone();
                tunnel_guard = Some(t);
                (Mode::Tunnel, url)
            }
            Err(error) => {
                eprintln!("! {error:#}");
                eprintln!("! Falling back to LAN mode with a self-signed certificate.");
                (Mode::LocalTls, format!("https://{lan_host}:{}", args.port))
            }
        }
    } else if args.http {
        (Mode::PlainHttp, format!("http://{lan_host}:{}", args.port))
    } else {
        (Mode::LocalTls, format!("https://{lan_host}:{}", args.port))
    };

    let invite_url = format!("{public_base}/j/{token}");
    let host_url = match mode {
        Mode::LocalTls => format!("http://127.0.0.1:{}/h/{host_key}", args.host_port),
        Mode::PlainHttp => format!("http://127.0.0.1:{}/h/{host_key}", args.port),
        Mode::Proxy | Mode::Tunnel => format!("{public_base}/h/{host_key}"),
    };

    let state = AppState::new(
        token,
        host_key,
        invite_url.clone(),
        media,
        mode.slug(),
        version.clone(),
    );
    let main_app = server::router(state.clone(), false);
    let host_app = server::router(state.clone(), true);

    print_banner(
        &version,
        mode,
        &host_url,
        &invite_url,
        &lan_ips,
        &args,
        state.media.is_relay_configured(),
    );

    let main_addr = match mode {
        Mode::Tunnel => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), args.port),
        _ => SocketAddr::new(args.bind, args.port),
    };

    match mode {
        Mode::Proxy | Mode::Tunnel | Mode::PlainHttp => {
            if let Mode::PlainHttp = mode {
                println!(
                    "  Warning: plain HTTP works only for guests on this machine (localhost is a secure\n  context). Remote guests cannot capture screens or use the camera over plain HTTP.\n"
                );
            }
            let server = axum_server::bind(main_addr).serve(main_app.into_make_service());
            tokio::select! {
                result = server => {
                    result.with_context(|| format!("serving on {main_addr}"))?;
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nSession ended.");
                }
            }
            drop(tunnel_guard);
        }
        Mode::LocalTls => {
            let mut sans = vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
                "triple.local".to_string(),
            ];
            for ip in &lan_ips {
                sans.push(ip.to_string());
            }
            sans.sort();
            sans.dedup();
            let cert_dir = args
                .cert_dir
                .clone()
                .unwrap_or_else(certs::default_cert_dir);
            let material = certs::ensure(&cert_dir, &sans)
                .with_context(|| format!("preparing certificate in {}", cert_dir.display()))?;
            let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
                material.cert_pem.into_bytes(),
                material.key_pem.into_bytes(),
            )
            .await
            .context("loading generated TLS certificate")?;

            let host_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), args.host_port);
            let tls_server =
                axum_server::bind_rustls(main_addr, tls).serve(main_app.into_make_service());
            let host_server = axum_server::bind(host_addr).serve(host_app.into_make_service());

            println!(
                "  Certificate: {}\n  Guests will see a one-time warning for this self-signed LAN certificate.\n  Accept it once; screen capture and camera need that secure context.\n",
                cert_dir.display()
            );
            println!("  Press Ctrl+C to end the session.\n");

            tokio::select! {
                result = tls_server => {
                    result.with_context(|| format!("serving TLS on {main_addr}"))?;
                }
                result = host_server => {
                    result.with_context(|| format!("serving host view on {host_addr}"))?;
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nSession ended.");
                }
            }
            drop(tunnel_guard);
        }
    }

    Ok(())
}

fn build_media(args: &Args) -> MediaConfig {
    let mut ice_servers = Vec::new();
    let stun: Vec<String> = args
        .stun
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if !stun.is_empty() {
        ice_servers.push(IceServer {
            urls: stun,
            username: None,
            credential: None,
        });
    }
    if let Some(url) = args
        .turn_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ice_servers.push(IceServer {
            urls: vec![url.to_string()],
            username: args.turn_user.clone().filter(|s| !s.is_empty()),
            credential: args.turn_pass.clone().filter(|s| !s.is_empty()),
        });
    }
    MediaConfig { ice_servers }
}

fn normalize_base(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn random_hex(bytes: usize) -> String {
    use rand::RngCore;
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    let mut out = String::with_capacity(bytes * 2);
    for byte in buffer {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn print_banner(
    version: &str,
    mode: Mode,
    host_url: &str,
    invite_url: &str,
    lan_ips: &[IpAddr],
    args: &Args,
    relay: bool,
) {
    println!();
    println!("  triple {version}");
    println!("  Mode: {}", mode.description());
    println!();
    println!("  Host view (open on this computer):");
    println!("    {host_url}");
    println!();
    println!("  Invite link (share with your pair):");
    println!("    {invite_url}");
    println!();

    if mode == Mode::LocalTls {
        println!("  Local addresses:");
        if lan_ips.is_empty() {
            println!("    (no LAN interface detected)");
        } else {
            for ip in lan_ips {
                println!("    https://{}:{}/j/...", net::url_host(*ip), args.port);
            }
        }
        println!();
    }
    if mode == Mode::Proxy {
        println!(
            "  Reverse proxy: forward / and WebSocket upgrades on /ws to {}:{}.\n  Public URL must route to this process.",
            args.bind, args.port
        );
        println!();
    }
    if mode == Mode::Tunnel {
        println!(
            "  cloudflared is running in the background. The public URL lives while triple runs."
        );
        println!();
    }
    println!(
        "  Media relay: {}",
        if relay {
            "TURN configured (relay available for strict NATs)"
        } else {
            "no TURN configured (direct + STUN only)"
        }
    );
    println!();
}
