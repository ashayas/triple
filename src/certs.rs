use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub struct TlsMaterial {
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn default_cert_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TRIPLE_CERT_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    if cfg!(target_os = "macos") {
        PathBuf::from(home).join("Library/Application Support/triple/certs")
    } else if cfg!(target_os = "windows") {
        let base = std::env::var("APPDATA").unwrap_or(home);
        PathBuf::from(base).join("triple").join("certs")
    } else {
        let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
        PathBuf::from(base).join("triple").join("certs")
    }
}

/// Load a cached certificate for these SANs, or generate and cache a new
/// self-signed one. The certificate is reused across runs so guests only have to
/// trust it once.
pub fn ensure(dir: &Path, sans: &[String]) -> Result<TlsMaterial> {
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    if cert_path.exists() && key_path.exists() {
        let cert_pem = fs::read_to_string(&cert_path)?;
        let key_pem = fs::read_to_string(&key_path)?;
        if cert_pem.contains("BEGIN CERTIFICATE") && key_pem.contains("PRIVATE KEY") {
            return Ok(TlsMaterial { cert_pem, key_pem });
        }
    }

    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let key_pair = rcgen::KeyPair::generate()?;
    let mut params =
        rcgen::CertificateParams::new(sans.to_vec()).context("building certificate parameters")?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "triple local session");
    params.is_ca = rcgen::IsCa::NoCa;
    params.not_before = rcgen::date_time_ymd(2025, 1, 1);
    params.not_after = rcgen::date_time_ymd(2038, 1, 1);
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    fs::write(&cert_path, &cert_pem)?;
    fs::write(&key_path, &key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
    }
    Ok(TlsMaterial { cert_pem, key_pem })
}
