use anyhow::{anyhow, Result};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub struct Tunnel {
    pub url: String,
    pub child: Child,
}

/// Start a cloudflared quick tunnel for `http://127.0.0.1:<port>` and wait for
/// it to print its public HTTPS URL.
pub async fn start_quick_tunnel(port: u16) -> Result<Tunnel> {
    let mut child = Command::new("cloudflared")
        .args([
            "tunnel",
            "--url",
            &format!("http://127.0.0.1:{port}"),
            "--no-autoupdate",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            anyhow!(
                "cloudflared is not installed or not on PATH ({e}). \
                 Install it from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/ \
                 or run triple without --tunnel."
            )
        })?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("cloudflared stderr unavailable"))?;
    let mut lines = BufReader::new(stderr).lines();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if let Some(url) = extract_tunnel_url(&line) {
                    return Ok(Tunnel { url, child });
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => break,
        }
    }

    let _ = child.kill().await;
    Err(anyhow!("cloudflared did not report a public URL in time"))
}

fn extract_tunnel_url(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        let candidate = word.trim_matches(|c: char| {
            matches!(c, '|' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';')
        });
        if let Some(idx) = candidate.find("https://") {
            let url = &candidate[idx..];
            if url.contains(".trycloudflare.com") {
                return Some(url.trim_end_matches('.').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_tunnel_url;

    #[test]
    fn parses_quick_tunnel_banner() {
        let line = "2026-01-01T00:00:00Z INF |  https://brave-blue-otter.trycloudflare.com  |";
        assert_eq!(
            extract_tunnel_url(line).as_deref(),
            Some("https://brave-blue-otter.trycloudflare.com")
        );
    }

    #[test]
    fn ignores_other_lines() {
        assert!(extract_tunnel_url("INF Registered tunnel connection").is_none());
    }
}
