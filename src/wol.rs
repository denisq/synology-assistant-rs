//! Wake-on-LAN: magic packets and the list of registered devices.

use serde::{Deserialize, Serialize};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WolDevice {
    pub name: String,
    pub mac: String,
    /// Last known IP (informational) — packets go to broadcast addresses.
    #[serde(default)]
    pub ip: String,
    /// Optional explicit target (directed broadcast or hostname), e.g. 192.168.1.255.
    #[serde(default)]
    pub target: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    9
}

pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let sep_ok = s
        .chars()
        .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '-' | '.' | ' '));
    if hex.len() != 12 || !sep_ok {
        return None;
    }
    let mut m = [0u8; 6];
    for (i, b) in m.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(m)
}

pub fn format_mac(m: &[u8; 6]) -> String {
    m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

pub fn magic_packet(mac: &[u8; 6]) -> Vec<u8> {
    let mut p = vec![0xffu8; 6];
    for _ in 0..16 {
        p.extend_from_slice(mac);
    }
    p
}

/// Send the magic packet to every broadcast address (and the explicit target, if any),
/// on the configured port and on port 7 as well.
pub fn wake(dev: &WolDevice) -> io::Result<usize> {
    let mac = parse_mac(&dev.mac).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid MAC address"))?;
    let pkt = magic_packet(&mac);
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_broadcast(true)?;
    let mut targets: Vec<Ipv4Addr> = crate::discovery::broadcast_targets();
    if let Ok(t) = dev.target.trim().parse::<Ipv4Addr>() {
        targets.insert(0, t);
    }
    let mut ports = vec![dev.port];
    if dev.port != 7 {
        ports.push(7);
    }
    let mut sent = 0;
    let mut last_err = None;
    for t in &targets {
        for &p in &ports {
            match sock.send_to(&pkt, SocketAddrV4::new(*t, p)) {
                Ok(_) => sent += 1,
                Err(e) => last_err = Some(e),
            }
        }
    }
    if !dev.target.trim().is_empty() && dev.target.trim().parse::<Ipv4Addr>().is_err() {
        use std::net::ToSocketAddrs;
        if let Ok(addrs) = (dev.target.trim(), dev.port).to_socket_addrs() {
            for a in addrs.filter(|a| a.is_ipv4()) {
                if sock.send_to(&pkt, a).is_ok() {
                    sent += 1;
                }
            }
        }
    }
    match (sent, last_err) {
        (0, Some(e)) => Err(e),
        _ => Ok(sent),
    }
}

fn store_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SynologyAssistantRS")
        .join("wol.json")
}

pub fn load() -> Vec<WolDevice> {
    std::fs::read(store_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(list: &[WolDevice]) -> io::Result<()> {
    let p = store_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serde_json::to_vec_pretty(list)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_parsing() {
        let m = [0x00, 0x11, 0x32, 0xab, 0xcd, 0xef];
        assert_eq!(parse_mac("00:11:32:ab:cd:ef"), Some(m));
        assert_eq!(parse_mac("00-11-32-AB-CD-EF"), Some(m));
        assert_eq!(parse_mac("001132abcdef"), Some(m));
        assert_eq!(parse_mac("00:11:32:ab:cd"), None);
        assert_eq!(parse_mac("zz:11:32:ab:cd:ef"), None);
        assert_eq!(format_mac(&m), "00:11:32:ab:cd:ef");
    }

    #[test]
    fn magic_packet_layout() {
        let m = [1, 2, 3, 4, 5, 6];
        let p = magic_packet(&m);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xff; 6]);
        assert_eq!(&p[96..], &m);
    }
}
