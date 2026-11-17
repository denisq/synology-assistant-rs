//! LAN discovery: broadcast the findhost query and collect replies on UDP 9999.
//! Also sends commands (memory test) and collects their acknowledgements.

use crate::protocol::{self, NasInfo};
use crate::secure::{self, Session};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Device {
    pub info: NasInfo,
    pub last_seen: Instant,
}

pub type DeviceMap = Arc<Mutex<BTreeMap<String, Device>>>;

/// Acknowledgement received for a command: (result code, when).
pub type AckMap = Arc<Mutex<HashMap<String, (u32, Instant)>>>;

pub struct Discovery {
    socket: Arc<UdpSocket>,
    session: Arc<Session>,
    pub devices: DeviceMap,
    pub acks: AckMap,
}

fn open_socket() -> io::Result<UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    // Share the port with other finders where the OS allows it.
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos", target_os = "cygwin"))))]
    s.set_reuse_port(true)?;
    s.set_broadcast(true)?;
    // NAS replies are broadcast to port 9999, so we must listen there.
    s.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, protocol::PORT)).into())?;
    Ok(s.into())
}

/// Broadcast addresses of every up, non-loopback IPv4 interface, plus 255.255.255.255.
pub fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut v = vec![Ipv4Addr::BROADCAST];
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        for i in ifs {
            if i.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(a) = i.addr {
                let b = a.broadcast.unwrap_or_else(|| Ipv4Addr::from(u32::from(a.ip) | !u32::from(a.netmask)));
                if !v.contains(&b) && !a.ip.is_link_local() {
                    v.push(b);
                }
            }
        }
    }
    v
}

impl Discovery {
    /// Start listening. `on_update` is called (from the receiver thread) whenever
    /// something new arrives.
    pub fn start(on_update: impl Fn() + Send + 'static) -> io::Result<Self> {
        let socket = Arc::new(open_socket()?);
        let session = Arc::new(Session::new());
        let devices: DeviceMap = Default::default();
        let acks: AckMap = Default::default();
        let (rx_sock, rx_devs, rx_acks, rx_sess) = (socket.clone(), devices.clone(), acks.clone(), session.clone());
        std::thread::Builder::new().name("findhost-rx".into()).spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let Ok((n, _from)) = rx_sock.recv_from(&mut buf) else {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                };
                let raw = &buf[..n];
                let plain;
                let pkt = if raw.starts_with(&secure::MAGIC_ENCRYPTED) {
                    // Sealed to our key, or to another client's: skip the latter.
                    let Some(p) = rx_sess.open(raw) else { continue };
                    plain = p;
                    &plain[..]
                } else {
                    raw
                };
                let Some(info) = protocol::parse(pkt) else { continue };
                if info.packet_type == protocol::PKT_ACK && !info.mac.is_empty() {
                    if let Some(code) = info.result_code {
                        rx_acks.lock().unwrap().insert(info.mac.clone(), (code, Instant::now()));
                        on_update();
                    }
                    continue;
                }
                if !info.is_device_info() {
                    continue;
                }
                let mut devs = rx_devs.lock().unwrap();
                let mut info = info;
                // Plain replies don't carry the key; keep the one we already learned.
                if let Some(old) = devs.get(&info.mac) {
                    if info.public_key.is_empty() {
                        info.public_key = old.info.public_key.clone();
                        info.session_id = info.session_id.or(old.info.session_id);
                    }
                }
                devs.insert(info.mac.clone(), Device { info, last_seen: Instant::now() });
                drop(devs);
                on_update();
            }
        })?;
        Ok(Self { socket, session, devices, acks })
    }

    fn broadcast(&self, pkt: &[u8]) {
        for b in broadcast_targets() {
            let _ = self.socket.send_to(pkt, SocketAddrV4::new(b, protocol::PORT));
        }
    }

    /// Send the discovery query (with our public key) on every interface.
    pub fn search(&self) {
        self.broadcast(&protocol::build_query(Some(&self.session)));
    }

    /// Ask `dev` to reboot into the memory test. The answer arrives in `acks`.
    /// The packet is sealed to the NAS's key when it advertised one, like the original app.
    pub fn start_memtest(&self, dev: &NasInfo, account: &str, password: &str) -> io::Result<()> {
        self.acks.lock().unwrap().remove(&dev.mac);
        let encrypted = !dev.public_key.is_empty() && dev.session_id.is_some();
        let plain = protocol::build_memtest(&dev.mac, account, password, encrypted.then_some(self.session.id));
        let pkt = if encrypted {
            secure::seal(&plain, &dev.public_key)
                .ok_or_else(|| io::Error::other("invalid NAS public key"))?
        } else {
            plain
        };
        // The original app sends the command a few times, one second apart.
        let sock = self.socket.clone();
        let targets = broadcast_targets();
        std::thread::spawn(move || {
            for i in 0..3 {
                if i > 0 {
                    std::thread::sleep(Duration::from_secs(1));
                }
                for b in &targets {
                    let _ = sock.send_to(&pkt, SocketAddrV4::new(*b, protocol::PORT));
                }
            }
        });
        Ok(())
    }

    pub fn ack(&self, mac: &str) -> Option<(u32, Instant)> {
        self.acks.lock().unwrap().get(mac).copied()
    }

    pub fn clear(&self) {
        self.devices.lock().unwrap().clear();
    }

    pub fn snapshot(&self) -> Vec<Device> {
        self.devices.lock().unwrap().values().cloned().collect()
    }
}

/// One-shot scan for the command-line mode.
pub fn scan_blocking(wait: Duration) -> io::Result<Vec<NasInfo>> {
    let d = Discovery::start(|| {})?;
    // Some devices (e.g. during a memory test) take ~1.5 s to answer.
    d.search();
    std::thread::sleep(Duration::from_secs(1));
    d.search();
    std::thread::sleep(wait.saturating_sub(Duration::from_secs(1)));
    Ok(d.snapshot().into_iter().map(|x| x.info).collect())
}
