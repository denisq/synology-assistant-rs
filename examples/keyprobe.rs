//! Diagnostic: send a query with our public key and dump every reply (decrypting sealed ones).
use std::net::UdpSocket;
use std::time::{Duration, Instant};
use synology_assistant_rs::{
    protocol,
    secure::{self, Session},
};

fn main() {
    let s = Session::new();
    let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None).unwrap();
    sock.set_reuse_address(true).unwrap();
    #[cfg(unix)]
    sock.set_reuse_port(true).unwrap();
    sock.set_broadcast(true).unwrap();
    sock.bind(&"0.0.0.0:9999".parse::<std::net::SocketAddr>().unwrap().into())
        .unwrap();
    let sock: UdpSocket = sock.into();
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let q = protocol::build_query(Some(&s));
    sock.send_to(&q, "255.255.255.255:9999").unwrap();
    let t0 = Instant::now();
    let end = t0 + Duration::from_secs(6);
    let mut buf = [0u8; 8192];
    while Instant::now() < end {
        let Ok((n, from)) = sock.recv_from(&mut buf) else {
            continue;
        };
        let raw = &buf[..n];
        if raw == q.as_slice() {
            continue;
        }
        let (kind, plain) = match s.open(raw) {
            Some(p) => ("ENCRYPTED->decrypted", p),
            None if raw.starts_with(&secure::MAGIC_ENCRYPTED) => ("ENCRYPTED (not for us)", raw.to_vec()),
            None => ("plain", raw.to_vec()),
        };
        let info = protocol::parse(&plain);
        println!(
            "{:?} {from} {n}B {kind}: {:?}",
            t0.elapsed(),
            info.map(|i| (
                i.packet_type,
                i.name,
                i.status_code,
                i.progress,
                i.version,
                i.session_id
            ))
        );
    }
}
