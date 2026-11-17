//! Checks whether this computer can actually reach a NAS it discovered.
//! Broadcast discovery works across IP subnets on the same cable or Wi-Fi, but
//! opening DSM does not, which is a classic source of confusion.

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
        return Some(format!(
            "The NAS has a self-assigned address ({nas}), so it got no address from DHCP. \
             Check its network cable and that your router hands out addresses."
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
    Some(format!(
        "The NAS ({nas}) is on a different subnet than this computer ({mine}), \
         so DSM may not open. Change the NAS's network settings or your computer's."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: [(Ipv4Addr, Ipv4Addr); 1] = [(Ipv4Addr::new(192, 168, 1, 20), Ipv4Addr::new(255, 255, 255, 0))];

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
