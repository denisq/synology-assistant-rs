//! `MatrixEncode` from the original app: the obfuscation Synology Assistant applies
//! to the admin password before putting it in a findhost packet.
//!
//! The string is split into rows of 8 bytes (zero padded), multiplied by a fixed
//! 8x8 integer matrix, and each resulting value is written as two characters
//! (low 6 bits, then bits 6..11) from a 64-character alphabet.

const M: [[i32; 8]; 8] = [
    [0, 2, 2, -3, 0, 1, 3, -1],
    [1, 1, -2, 3, -1, 0, 0, 5],
    [-2, 1, 1, -1, 3, 0, -1, -2],
    [-1, 0, 0, 0, 2, -3, -4, 1],
    [0, -2, 1, 2, -2, 1, -2, -1],
    [-1, 2, 0, 2, -2, -2, 1, 0],
    [2, -4, 3, -2, 1, 5, 3, 1],
    [1, 0, -5, 0, -1, -2, -1, -3],
];

const ALPHABET: &[u8; 64] = b"UPX-BkYa4Fyi2DjcLef6WmOA8pZrshQ+uv7Vwx3G9oHb1EIJKzMg5NqRSCtTld0n";

pub fn encode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(8) * 16);
    for chunk in bytes.chunks(8) {
        let mut row = [0i32; 8];
        for (i, &b) in chunk.iter().enumerate() {
            row[i] = b as i32;
        }
        for col in 0..8 {
            let v: i32 = (0..8).map(|k| row[k] * M[k][col]).sum();
            let v = v as u32;
            out.push(ALPHABET[(v & 0x3f) as usize] as char);
            out.push(ALPHABET[((v >> 6) & 0x3f) as usize] as char);
        }
    }
    out
}

/// Inverse of [`encode`] (used only to self-check the encoder).
#[cfg(test)]
pub fn decode(s: &str) -> String {
    // Integer inverse via Gaussian elimination over rationals is overkill; solve with f64.
    let inv = invert();
    let idx = |c: u8| ALPHABET.iter().position(|&a| a == c).unwrap() as i32;
    let b = s.as_bytes();
    let mut out = Vec::new();
    for row in b.chunks(16) {
        let mut v = [0f64; 8];
        for col in 0..8 {
            let mut x = idx(row[col * 2]) | (idx(row[col * 2 + 1]) << 6);
            if x & 0x800 != 0 {
                x |= !0xfff;
            }
            v[col] = x as f64;
        }
        for col in 0..8 {
            let c: f64 = (0..8).map(|k| v[k] * inv[k][col]).sum();
            let c = c.round() as u8;
            if c != 0 {
                out.push(c);
            }
        }
    }
    String::from_utf8(out).unwrap()
}

#[cfg(test)]
fn invert() -> [[f64; 8]; 8] {
    let mut a = [[0f64; 16]; 8];
    for i in 0..8 {
        for j in 0..8 {
            a[i][j] = M[i][j] as f64;
        }
        a[i][8 + i] = 1.0;
    }
    for c in 0..8 {
        let p = (c..8).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs())).unwrap();
        a.swap(c, p);
        let d = a[c][c];
        for j in 0..16 {
            a[c][j] /= d;
        }
        for r in 0..8 {
            if r != c {
                let f = a[r][c];
                for j in 0..16 {
                    a[r][j] -= f * a[c][j];
                }
            }
        }
    }
    let mut inv = [[0f64; 8]; 8];
    for i in 0..8 {
        inv[i].copy_from_slice(&a[i][8..]);
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_shape() {
        for s in ["a", "password", "Sup3r-Secret!", "ünïcödé", ""] {
            let e = encode(s);
            assert_eq!(e.len(), s.len().div_ceil(8) * 16);
            assert_eq!(decode(&e), s);
        }
    }

    #[test]
    fn known_vector() {
        // "a" (97) -> row [97,0,..] x M = first matrix row * 97.
        let e = encode("a");
        assert_eq!(&e[0..2], "UU"); // 0
        // 2*97 = 194 = 0b11_000010 -> low 6 = 2 ('X'), high = 3 ('-')
        assert_eq!(&e[2..4], "X-");
    }
}
