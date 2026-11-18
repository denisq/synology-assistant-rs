//! Checks whether this computer can actually reach a NAS it discovered.
//! Broadcast discovery works across IP subnets on the same cable or Wi-Fi, but
//! opening DSM does not, which is a classic source of confusion.

use crate::t;
use std::net::Ipv4Addr;

/// IPv4 address and netmask of each active, non-loopback local interface.
pub fn local_networks() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(a) => Some((a.ip, a.netmask)),
            _ => None,
        })
        .collect()
}

/// One IPv4 interface of this computer.
#[derive(Debug, Clone, PartialEq)]
pub struct Iface {
    pub name: String,
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
}

pub fn interfaces() -> Vec<Iface> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(a) => Some(Iface {
                name: i.name,
                ip: a.ip,
                mask: a.netmask,
            }),
            _ => None,
        })
        .collect()
}

/// Interface names used by VPN clients (macOS utun, Linux tun/wg/ppp, Windows adapter names).
pub fn is_vpn_interface(name: &str) -> bool {
    let n = name.to_lowercase();
    ["utun", "tun", "tap", "wg", "ppp", "ipsec", "gpd"]
        .iter()
        .any(|p| n.starts_with(p))
        || [
            "vpn",
            "wireguard",
            "anyconnect",
            "globalprotect",
            "fortinet",
            "nordlynx",
            "openvpn",
            "tailscale",
            "zerotier",
        ]
        .iter()
        .any(|k| n.contains(k))
}

/// Problems with this computer's network that would stop discovery, worst first.
pub fn diagnose(ifaces: &[Iface]) -> Vec<String> {
    let mut issues = Vec::new();
    let vpn: Vec<&Iface> = ifaces.iter().filter(|i| is_vpn_interface(&i.name)).collect();
    let lan: Vec<&Iface> = ifaces
        .iter()
        .filter(|i| !is_vpn_interface(&i.name) && !i.ip.is_link_local())
        .collect();
    if lan.is_empty() {
        if ifaces.iter().any(|i| i.ip.is_link_local()) {
            issues.push(
                t!(
                    "This computer has only a self-assigned address (169.254.x.x): it got no address from your router. \
                 Check the cable or Wi-Fi connection."
                )
                .to_string(),
            );
        } else {
            issues.push(t!("This computer is not connected to a local network.").to_string());
        }
    }
    if !vpn.is_empty() {
        let names = vpn
            .iter()
            .map(|i| format!("{} ({})", i.name, i.ip))
            .collect::<Vec<_>>()
            .join(", ");
        issues.push(t!(
            "A VPN seems to be active: {names}. Many VPNs block or reroute local network traffic, \
             which hides your NAS. Disconnect it, or allow local network access in the VPN's settings.",
            names = names
        ));
    }
    issues
}

/// Which DSM web port answers on `ip`, if any (TCP connect with a short timeout).
pub fn probe_dsm(ip: Ipv4Addr, ports: &[u16]) -> Option<u16> {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    ports
        .iter()
        .copied()
        .find(|&p| TcpStream::connect_timeout(&SocketAddr::from((ip, p)), Duration::from_millis(800)).is_ok())
}

/// Check a manual IPv4 configuration before sending it to a NAS.
pub fn validate_static(ip: Ipv4Addr, mask: Ipv4Addr, gateway: Ipv4Addr) -> Result<(), String> {
    let m = u32::from(mask);
    if m == 0 || m.leading_ones() + m.trailing_zeros() != 32 {
        return Err(t!("{mask} is not a valid subnet mask.", mask = mask));
    }
    if m != u32::MAX {
        let host = u32::from(ip) & !m;
        if host == 0 || host == !m {
            return Err(t!("{ip} is the network or broadcast address of that subnet.", ip = ip));
        }
    }
    if ip == gateway {
        return Err(t!("The IP address and the gateway must be different.").into());
    }
    if !same_subnet(ip, gateway, mask) {
        return Err(t!(
            "The gateway {gateway} is not in the same subnet as {ip}/{prefix}.",
            gateway = gateway,
            ip = ip,
            prefix = prefix_len(mask)
        ));
    }
    Ok(())
}

fn same_subnet(a: Ipv4Addr, b: Ipv4Addr, mask: Ipv4Addr) -> bool {
    let m = u32::from(mask);
    u32::from(a) & m == u32::from(b) & m
}

fn prefix_len(mask: Ipv4Addr) -> u32 {
    u32::from(mask).count_ones()
}

/// A warning when the NAS is unlikely to be reachable from here, else `None`.
pub fn reachability_warning(nas: Ipv4Addr, local: &[(Ipv4Addr, Ipv4Addr)]) -> Option<String> {
    if nas.is_link_local() {
        return Some(t!(
            "The NAS has a self-assigned address ({nas}), so it got no address from DHCP. \
             Check its network cable and that your router hands out addresses.",
            nas = nas
        ));
    }
    if local.is_empty() || local.iter().any(|&(ip, mask)| same_subnet(ip, nas, mask)) {
        return None;
    }
    let mine = local
        .iter()
        .filter(|(ip, _)| !ip.is_link_local())
        .map(|(ip, mask)| format!("{ip}/{}", prefix_len(*mask)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(t!(
        "The NAS ({nas}) is on a different subnet than this computer ({mine}), \
         so DSM may not open. Change the NAS's network settings or your computer's.",
        nas = nas,
        mine = mine
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: [(Ipv4Addr, Ipv4Addr); 1] = [(Ipv4Addr::new(192, 168, 1, 20), Ipv4Addr::new(255, 255, 255, 0))];

    fn iface(name: &str, ip: [u8; 4]) -> Iface {
        Iface {
            name: name.into(),
            ip: Ipv4Addr::from(ip),
            mask: Ipv4Addr::new(255, 255, 255, 0),
        }
    }

    #[test]
    fn diagnose_vpn_and_missing_lan() {
        assert!(diagnose(&[iface("en0", [192, 168, 1, 20])]).is_empty());
        let d = diagnose(&[iface("en0", [192, 168, 1, 20]), iface("utun3", [10, 8, 0, 2])]);
        assert_eq!(d.len(), 1);
        assert!(d[0].contains("VPN") && d[0].contains("utun3"));
        assert!(diagnose(&[]).iter().any(|m| m.contains("not connected")));
        assert!(
            diagnose(&[iface("en0", [169, 254, 1, 2])])
                .iter()
                .any(|m| m.contains("self-assigned"))
        );
        assert!(is_vpn_interface("WireGuard Tunnel") && !is_vpn_interface("en0") && !is_vpn_interface("Ethernet"));
    }

    #[test]
    fn static_config_validation() {
        let v = |ip: [u8; 4], m: [u8; 4], g: [u8; 4]| validate_static(ip.into(), m.into(), g.into());
        assert!(v([192, 168, 1, 50], [255, 255, 255, 0], [192, 168, 1, 1]).is_ok());
        assert!(v([192, 168, 1, 50], [255, 0, 255, 0], [192, 168, 1, 1]).is_err()); // non-contiguous mask
        assert!(v([192, 168, 1, 255], [255, 255, 255, 0], [192, 168, 1, 1]).is_err()); // broadcast
        assert!(v([192, 168, 1, 0], [255, 255, 255, 0], [192, 168, 1, 1]).is_err()); // network
        assert!(v([192, 168, 1, 50], [255, 255, 255, 0], [192, 168, 2, 1]).is_err()); // gateway elsewhere
        assert!(v([192, 168, 1, 1], [255, 255, 255, 0], [192, 168, 1, 1]).is_err()); // same as gateway
    }

    #[test]
    fn same_network_is_fine() {
        assert_eq!(reachability_warning(Ipv4Addr::new(192, 168, 1, 15), &HOME), None);
    }

    #[test]
    fn other_subnet_and_link_local_warn() {
        let w = reachability_warning(Ipv4Addr::new(192, 168, 2, 15), &HOME).unwrap();
        assert!(w.contains("different subnet") && w.contains("192.168.1.20/24"));
        let w = reachability_warning(Ipv4Addr::new(169, 254, 3, 4), &HOME).unwrap();
        assert!(w.contains("self-assigned"));
    }
}
