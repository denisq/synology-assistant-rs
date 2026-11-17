//! Synology "findhost" UDP protocol (port 9999).
//!
//! Reverse-engineered from the original Synology Assistant (DSAssistant, Qt/x86_64):
//! every packet is an 8-byte magic header followed by TLV records
//! `[id: u8][len: u8][value: len bytes]`. Integers are little-endian, IPv4
//! addresses are stored in network byte order, strings are plain ASCII/UTF-8
//! without terminator.

use std::net::Ipv4Addr;

pub const PORT: u16 = 9999;
pub const MAGIC: [u8; 8] = [0x12, 0x34, 0x56, 0x78, b'S', b'Y', b'N', b'O'];

/// Field IDs (from the `grgfieldAttribs` table in the original binary).
pub mod field {
    pub const PACKET_TYPE: u8 = 0x01;
    pub const SERVER_NAME: u8 = 0x11;
    pub const IP: u8 = 0x12;
    pub const NETMASK: u8 = 0x13;
    pub const DNS: u8 = 0x14;
    pub const GATEWAY: u8 = 0x15;
    pub const DHCP: u8 = 0x18;
    pub const MAC: u8 = 0x19;
    pub const BUILD: u8 = 0x49;
    pub const UNIQUE: u8 = 0x70;
    pub const HTTP_PORT: u8 = 0x75;
    pub const HTTPS_PORT: u8 = 0x76;
    pub const VERSION: u8 = 0x77;
    pub const MODEL: u8 = 0x78;
    pub const PROGRESS: u8 = 0x79;
    pub const MAGIC_A4: u8 = 0xa4;
    /// 64-bit "services supported" / "services running" bitmaps.
    pub const SERV_SUPPORTED: u8 = 0xb0;
    pub const SERV_RUNNING: u8 = 0xb8;
    pub const PROTO_VERSION: u8 = 0xa6;
    pub const SYS_STATUS: u8 = 0xa7;
    pub const SERIAL: u8 = 0xc0;
    pub const OS_NAME: u8 = 0xc1;
    pub const RESULT: u8 = 0x1f;
    pub const PASSWORD: u8 = 0x2a;
    pub const ACCOUNT: u8 = 0x4a;
    pub const CLIENT_BUILD: u8 = 0xc2;
    pub const PUBLIC_KEY: u8 = 0xc4;
    pub const SESSION_ID: u8 = 0xc5;
}

/// Packet type sent by the client when it searches the network.
pub const PKT_QUERY: u32 = 1;
/// Packet type of a full information reply from a NAS.
pub const PKT_INFO: u32 = 6;
/// Acknowledgement of a command, with a result code in field 0x1f.
pub const PKT_ACK: u32 = 2;
/// "Start memory test" command (`CFindHostUDP::sendPktMemTest`).
pub const PKT_MEMTEST: u32 = 0x0c;
/// Build number the original app (7.0.7-50095) reports; NASes may check it.
pub const CLIENT_BUILD: u32 = 50095;

fn header(packet_type: u32) -> Vec<u8> {
    let mut p = MAGIC.to_vec();
    push_u32(&mut p, field::MAGIC_A4, 0x0102_0000);
    push_u32(&mut p, field::PROTO_VERSION, 0x78);
    push_u32(&mut p, field::PACKET_TYPE, packet_type);
    p
}

/// Build the broadcast discovery query, byte-for-byte what DSAssistant sends.
/// With a session, our public key and session ID are appended so that an
/// encryption-capable NAS answers with its own key.
pub fn build_query(session: Option<&crate::secure::Session>) -> Vec<u8> {
    let mut p = header(PKT_QUERY);
    if let Some(s) = session {
        push_str(&mut p, field::PUBLIC_KEY, &s.public_hex());
        push_u32(&mut p, field::SESSION_ID, s.id);
    }
    p
}

/// Plain "start memory test" packet for the NAS with `mac`, authenticated with a
/// DSM administrator account. The password is obfuscated with `MatrixEncode`,
/// exactly as the original app does.
pub fn build_memtest(mac: &str, account: &str, password: &str, session_id: Option<u32>) -> Vec<u8> {
    let mut p = header(PKT_MEMTEST);
    push_str(&mut p, field::MAC, mac);
    let enc = crate::matrix::encode(password);
    if !enc.is_empty() {
        push_str(&mut p, field::PASSWORD, &enc);
    }
    push_str(&mut p, field::ACCOUNT, account);
    push_u32(&mut p, field::CLIENT_BUILD, CLIENT_BUILD);
    if let Some(id) = session_id {
        push_u32(&mut p, field::SESSION_ID, id);
    }
    p
}

fn push_str(p: &mut Vec<u8>, id: u8, v: &str) {
    let b = &v.as_bytes()[..v.len().min(255)];
    p.push(id);
    p.push(b.len() as u8);
    p.extend_from_slice(b);
}

fn push_u32(p: &mut Vec<u8>, id: u8, v: u32) {
    p.push(id);
    p.push(4);
    p.extend_from_slice(&v.to_le_bytes());
}

/// What a NAS reports about itself. Serialized to remember known devices;
/// session-specific fields are not stored.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct NasInfo {
    pub packet_type: u32,
    pub name: String,
    pub mac: String,
    pub ip: Option<Ipv4Addr>,
    pub netmask: Option<Ipv4Addr>,
    pub gateway: Option<Ipv4Addr>,
    pub dns: Option<Ipv4Addr>,
    pub dhcp: Option<bool>,
    pub model: String,
    pub unique: String,
    pub os_name: String,
    pub version: String,
    pub build: Option<u32>,
    pub serial: String,
    pub status_code: Option<u32>,
    pub progress: Option<u32>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    /// NAS public key (hex) when it supports encrypted commands.
    #[serde(skip)]
    pub public_key: String,
    #[serde(skip)]
    pub session_id: Option<u32>,
    pub serv_supported: Option<u64>,
    pub serv_running: Option<u64>,
    /// Result code of an acknowledgement (packet type 2), field 0x1f.
    #[serde(skip)]
    pub result_code: Option<u32>,
}

fn u32_le(v: &[u8]) -> Option<u32> {
    (v.len() >= 4).then(|| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn u64_le(v: &[u8]) -> Option<u64> {
    v.get(..8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

fn ipv4(v: &[u8]) -> Option<Ipv4Addr> {
    (v.len() == 4).then(|| Ipv4Addr::new(v[0], v[1], v[2], v[3]))
}

fn text(v: &[u8]) -> String {
    let end = v.iter().position(|&b| b == 0).unwrap_or(v.len());
    String::from_utf8_lossy(&v[..end]).trim().to_string()
}

/// Parse one findhost packet. Returns `None` if it is not a findhost packet.
pub fn parse(buf: &[u8]) -> Option<NasInfo> {
    let body = buf.strip_prefix(&MAGIC)?;
    let mut n = NasInfo::default();
    let mut i = 0;
    while i + 2 <= body.len() {
        let id = body[i];
        let len = body[i + 1] as usize;
        let Some(v) = body.get(i + 2..i + 2 + len) else { break };
        i += 2 + len;
        match id {
            field::PACKET_TYPE => n.packet_type = u32_le(v).unwrap_or(0),
            field::SERVER_NAME => n.name = text(v),
            field::MAC => n.mac = text(v).to_lowercase(),
            field::IP => n.ip = ipv4(v),
            field::NETMASK => n.netmask = ipv4(v),
            field::GATEWAY => n.gateway = ipv4(v),
            field::DNS => n.dns = ipv4(v),
            field::DHCP => n.dhcp = u32_le(v).map(|x| x != 0),
            field::MODEL => n.model = text(v),
            field::UNIQUE => n.unique = text(v),
            field::OS_NAME => n.os_name = text(v),
            field::VERSION => n.version = text(v),
            field::BUILD => n.build = u32_le(v),
            field::SERIAL => n.serial = text(v),
            field::SYS_STATUS => n.status_code = u32_le(v),
            field::PROGRESS => n.progress = u32_le(v),
            field::HTTP_PORT => n.http_port = u32_le(v).and_then(|x| u16::try_from(x).ok()).filter(|&p| p != 0),
            field::HTTPS_PORT => n.https_port = u32_le(v).and_then(|x| u16::try_from(x).ok()).filter(|&p| p != 0),
            field::SERV_SUPPORTED => n.serv_supported = u64_le(v),
            field::SERV_RUNNING => n.serv_running = u64_le(v),
            field::PUBLIC_KEY => n.public_key = text(v),
            field::RESULT => n.result_code = u32_le(v),
            field::SESSION_ID => n.session_id = u32_le(v).filter(|&x| x != 0),
            _ => {}
        }
    }
    Some(n)
}

impl NasInfo {
    /// True for packets worth showing in the device list (a NAS describing itself).
    pub fn is_device_info(&self) -> bool {
        self.packet_type == PKT_INFO && !self.mac.is_empty() && self.ip.is_some()
    }

    /// "7.4.1-90080" style version string.
    pub fn dsm_version(&self) -> String {
        match (self.version.is_empty(), self.build) {
            (false, Some(b)) => format!("{}-{}", self.version, b),
            (false, None) => self.version.clone(),
            (true, Some(b)) => format!("build {b}"),
            (true, None) => String::new(),
        }
    }

    /// Human-readable status, same wording as the original app.
    ///
    /// Code table recovered from `CAsstNasinfoModel::getServerStatus`; while a
    /// memory test runs the NAS reports code 9 with progress in hundredths of a percent.
    pub fn status(&self) -> String {
        let Some(code) = self.status_code else {
            return "Unknown".into();
        };
        let pct = self.progress.map(|p| format!("{}.{:02}%", p / 100, p % 100));
        match code {
            0 => "Not configured".into(),
            1 => "Ready".into(),
            2 => "Not installed".into(),
            3 => "Upgrading".into(),
            4 => "Configuration lost".into(),
            5 => "Booting".into(),
            6 if self.is_fsck_running() => match self.progress {
                Some(m) => format!("Checking file system ({m} min remaining)"),
                None => "Checking file system".into(),
            },
            6 => "Checking quota".into(),
            7 => "Starting services".into(),
            9 => format!("Performing memory test ({})", pct.unwrap_or_default()),
            11 => "Recoverable".into(),
            13 => "Check progress".into(),
            14 => "Migratable".into(),
            other => format!("Status {other}"),
        }
    }

    /// File-system check in progress: status 6 with service bit 8 supported and
    /// running (`CAsstNasinfoModel::getServerStatus`); field 0x79 holds the minutes left.
    pub fn is_fsck_running(&self) -> bool {
        const FSCK: u64 = 0x100;
        self.status_code == Some(6)
            && self.serv_supported.unwrap_or(0) & FSCK != 0
            && self.serv_running.unwrap_or(0) & FSCK != 0
    }

    pub fn is_memtest_running(&self) -> bool {
        self.status_code == Some(9)
    }

    pub fn web_url(&self, https: bool) -> Option<String> {
        let ip = self.ip?;
        Some(if https {
            format!("https://{ip}:{}", self.https_port.unwrap_or(5001))
        } else {
            format!("http://{ip}:{}", self.http_port.unwrap_or(5000))
        })
    }
}

/// Meaning of the result code in the memory-test acknowledgement
/// (`CAsstWizardMemTest::slotPktRecvReboot`).
pub fn memtest_result_message(code: u32) -> Result<(), &'static str> {
    match code {
        0 => Ok(()),
        1 => Err("The account or password is invalid. Please try again."),
        6 => Err("You are not authorized to use this service (login blocked)."),
        8 => Err("The user account has been disabled."),
        9 => Err("Please enter an administrator account."),
        _ => Err("The NAS refused to start the memory test."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Real reply from a DS220+ (DSM 7.4.1-90080) during a memory test.
    const DS220: &str = "1234567853594e4f11094453323230706c7573191130303a31313a33323a30303a30303a30311204c0a8010f1004010000001304ffffff001504c0a801011404c0a80101a30400000000010406000000a40400000201a60478000000480401000000780644533232302b701873796e6f6c6f67795f67656d696e696c616b655f3232302bc10344534d710401000000a7040900000079040f0f0000b0080000000000000000b80800000000000000009004000000004904e05f01007705372e342e31c00d30303030414243444546474849730a30304142434445464748";

    #[test]
    fn query_matches_original_app() {
        assert_eq!(
            build_query(None),
            hex("1234567853594e4fa40400000201a60478000000010401000000")
        );
    }

    #[test]
    fn parses_real_reply() {
        let n = parse(&hex(DS220)).unwrap();
        assert!(n.is_device_info());
        assert_eq!(n.name, "DS220plus");
        assert_eq!(n.mac, "00:11:32:00:00:01");
        assert_eq!(n.ip, Some(Ipv4Addr::new(192, 168, 1, 15)));
        assert_eq!(n.netmask, Some(Ipv4Addr::new(255, 255, 255, 0)));
        assert_eq!(n.model, "DS220+");
        assert_eq!(n.dsm_version(), "7.4.1-90080");
        assert_eq!(n.serial, "0000ABCDEFGHI");
        // Captured while a memory test was running (progress 0x0f0f = 38.55%).
        assert_eq!(n.status(), "Performing memory test (38.55%)");
        assert!(n.is_memtest_running());
        assert_eq!(n.web_url(false).unwrap(), "http://192.168.1.15:5000");
    }

    #[test]
    fn fsck_status() {
        let mut n = NasInfo {
            status_code: Some(6),
            ..Default::default()
        };
        assert_eq!(n.status(), "Checking quota");
        n.serv_supported = Some(0x100);
        n.serv_running = Some(0x100);
        n.progress = Some(12);
        assert_eq!(n.status(), "Checking file system (12 min remaining)");
    }

    #[test]
    fn memtest_packet_layout() {
        let p = build_memtest("00:11:32:00:00:01", "admin", "secret", Some(7));
        let n = parse(&p).unwrap();
        assert_eq!(n.packet_type, PKT_MEMTEST);
        assert_eq!(n.mac, "00:11:32:00:00:01");
        assert_eq!(n.session_id, Some(7));
        // Field order as in sendPktMemTest: a4, a6, 01, 19, 2a, 4a, c2, c5.
        let mut ids = vec![];
        let mut i = 8;
        while i < p.len() {
            ids.push(p[i]);
            i += 2 + p[i + 1] as usize;
        }
        assert_eq!(ids, [0xa4, 0xa6, 0x01, 0x19, 0x2a, 0x4a, 0xc2, 0xc5]);
        let pw = crate::matrix::encode("secret");
        assert!(p.windows(pw.len()).any(|w| w == pw.as_bytes()));
        assert!(!p.windows(6).any(|w| w == b"secret"));
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(parse(b"hello").is_none());
        let mut p = hex(DS220);
        p.truncate(40);
        let n = parse(&p).unwrap();
        assert_eq!(n.name, "DS220plus");
        assert!(!n.is_device_info());
    }
}
