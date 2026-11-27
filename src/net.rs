use std::net::IpAddr;

pub fn all_lan_ips() -> Vec<IpAddr> {
    let Ok(interfaces) = local_ip_address::list_afinet_netifas() else {
        return Vec::new();
    };
    let mut out: Vec<IpAddr> = interfaces
        .into_iter()
        .map(|(_name, ip)| ip)
        .filter(|ip| match ip {
            IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified(),
            IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified() && !v6.is_unique_local(),
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn preferred_ip() -> Option<IpAddr> {
    let ips = all_lan_ips();
    ips.iter()
        .find(|ip| ip.is_ipv4())
        .copied()
        .or_else(|| ips.first().copied())
        .or_else(|| local_ip_address::local_ip().ok())
}

/// Host portion for a URL, with IPv6 brackets.
pub fn url_host(ip: IpAddr) -> String {
    match ip {
        IpAddr::V6(v6) => format!("[{v6}]"),
        IpAddr::V4(v4) => v4.to_string(),
    }
}
