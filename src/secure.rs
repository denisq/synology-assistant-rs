//! Encrypted findhost packets (newer DSM).
//!
//! The client sends its X25519 public key (hex, field 0xc4) and a random session ID
//! (field 0xc5) with every query. A NAS that supports encryption answers with its
//! own key and ID; commands to that NAS are then sent as
//! `12 34 55 66 'SYNO' || crypto_box_seal(plain_packet, nas_public_key)`, and the NAS
//! may answer the same way, sealed to our key.

use crypto_box::{PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};

pub const MAGIC_ENCRYPTED: [u8; 8] = [0x12, 0x34, 0x55, 0x66, b'S', b'Y', b'N', b'O'];

pub struct Session {
    secret: SecretKey,
    pub id: u32,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        let mut id = 0;
        while id == 0 {
            id = OsRng.next_u32();
        }
        Self {
            secret: SecretKey::generate(&mut OsRng),
            id,
        }
    }

    pub fn public_hex(&self) -> String {
        hex(self.secret.public_key().as_bytes())
    }

    /// Decrypt a packet sealed to our key; returns the inner plain packet.
    pub fn open(&self, buf: &[u8]) -> Option<Vec<u8>> {
        let sealed = buf.strip_prefix(&MAGIC_ENCRYPTED)?;
        self.secret.unseal(sealed).ok()
    }
}

/// Seal a plain packet (including its normal header) to the NAS's public key.
pub fn seal(plain: &[u8], peer_key_hex: &str) -> Option<Vec<u8>> {
    let key = unhex(peer_key_hex)?;
    let key: [u8; 32] = key.try_into().ok()?;
    let sealed = PublicKey::from(key).seal(&mut OsRng, plain).ok()?;
    let mut out = MAGIC_ENCRYPTED.to_vec();
    out.extend_from_slice(&sealed);
    Some(out)
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open() {
        let s = Session::new();
        let pkt = b"\x12\x34\x56\x78SYNOhello";
        let sealed = seal(pkt, &s.public_hex()).unwrap();
        assert_eq!(&sealed[..8], &MAGIC_ENCRYPTED);
        assert_eq!(s.open(&sealed).unwrap(), pkt);
    }
}
