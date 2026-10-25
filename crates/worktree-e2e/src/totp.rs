//! TOTP code generator (RFC 6238: HMAC-SHA-1, 6 digits, 30 s steps) for the
//! MFA scenarios, with a tiny std-only SHA-1 / HMAC / base32 implementation
//! (the server side is specified in ENGINEERING_PLAN §12.2).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds per TOTP step.
pub const STEP_SECONDS: u64 = 30;
/// Digits per code.
pub const DIGITS: u32 = 6;

/// SHA-1 digest of `data` (FIPS 180-4).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// HMAC-SHA-1 (RFC 2104).
pub fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(BLOCK + msg.len());
    inner.extend(k.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(msg);
    let inner_hash = sha1(&inner);
    let mut outer = Vec::with_capacity(BLOCK + 20);
    outer.extend(k.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&inner_hash);
    sha1(&outer)
}

/// Decode RFC 4648 base32 (case-insensitive; `=` padding, spaces and dashes
/// are ignored). None on an invalid character.
pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    for c in s.chars() {
        let v = match c.to_ascii_uppercase() {
            c @ 'A'..='Z' => c as u64 - 'A' as u64,
            c @ '2'..='7' => c as u64 - '2' as u64 + 26,
            '=' | ' ' | '-' => continue,
            _ => return None,
        };
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1u64 << bits) - 1;
        }
    }
    Some(out)
}

/// Encode bytes as unpadded RFC 4648 base32 (upper case).
pub fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    for &b in data {
        buffer = (buffer << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1u64 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// HOTP value (RFC 4226) with `digits` digits, zero-padded.
pub fn hotp(key: &[u8], counter: u64, digits: u32) -> String {
    let mac = hmac_sha1(key, &counter.to_be_bytes());
    let offset = (mac[19] & 0x0f) as usize;
    let bin = ((mac[offset] as u32 & 0x7f) << 24)
        | ((mac[offset + 1] as u32) << 16)
        | ((mac[offset + 2] as u32) << 8)
        | (mac[offset + 3] as u32);
    let code = bin % 10u32.pow(digits);
    format!("{code:0width$}", width = digits as usize)
}

/// Current Unix time in seconds.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// The TOTP step containing `unix_secs`.
pub fn step_at(unix_secs: u64) -> u64 {
    unix_secs / STEP_SECONDS
}

/// Current TOTP step.
pub fn current_step() -> u64 {
    step_at(unix_now())
}

/// Decode a base32 secret (panics on garbage).
#[track_caller]
pub fn decode_secret(secret_b32: &str) -> Vec<u8> {
    base32_decode(secret_b32)
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| panic!("invalid base32 TOTP secret {secret_b32:?}"))
}

/// 6-digit code for `step`.
#[track_caller]
pub fn code_for_step(secret_b32: &str, step: u64) -> String {
    hotp(&decode_secret(secret_b32), step, DIGITS)
}

/// 6-digit code for the step containing `unix_secs`.
#[track_caller]
pub fn code_at(secret_b32: &str, unix_secs: u64) -> String {
    code_for_step(secret_b32, step_at(unix_secs))
}

/// 6-digit code for now.
#[track_caller]
pub fn code_now(secret_b32: &str) -> String {
    code_for_step(secret_b32, current_step())
}

/// Extract the base32 `secret` parameter of an `otpauth://` URL anywhere in
/// `text` (the URL may be percent-encoded around it).
pub fn secret_from_otpauth(text: &str) -> Option<String> {
    let start = text.find("otpauth://")?;
    let url = &text[start..];
    let url = url.split(char::is_whitespace).next().unwrap_or(url);
    let pos = url.find("secret=")?;
    let secret: String = url[pos + "secret=".len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '=')
        .collect();
    let secret = secret.trim_end_matches('=').to_string();
    base32_decode(&secret)
        .filter(|k| !k.is_empty())
        .map(|_| secret)
}

/// A TOTP authenticator that never hands out a code for a step it already
/// used (the server is replay-protected, §12.2) and stays within the
/// server's one step of skew: it uses the current step when unused, else the
/// next one, else waits for the clock.
#[derive(Debug, Clone)]
pub struct Totp {
    pub secret: String,
    last_step: Option<u64>,
}

impl Totp {
    #[track_caller]
    pub fn new(secret_b32: &str) -> Self {
        decode_secret(secret_b32);
        Self {
            secret: secret_b32.to_string(),
            last_step: None,
        }
    }

    /// Mark `step` as consumed (e.g. by a code sent outside this helper).
    pub fn mark_used(&mut self, step: u64) {
        self.last_step = Some(self.last_step.map_or(step, |s| s.max(step)));
    }

    /// A fresh code: the earliest unused step in `[now, now + 1]`, waiting
    /// for the clock when both are used. Blocks at most ~30 s.
    pub fn next_code(&mut self) -> String {
        loop {
            let now = current_step();
            let candidate = match self.last_step {
                Some(last) if last >= now => last + 1,
                _ => now,
            };
            if candidate <= now + 1 {
                // Only use the next step with a little margin left in the
                // current one, so the server's skew window surely covers it.
                let into_step = unix_now() % STEP_SECONDS;
                if candidate == now || into_step < STEP_SECONDS - 2 {
                    self.last_step = Some(candidate);
                    return code_for_step(&self.secret, candidate);
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// A syntactically valid code that is wrong for every step within ±5
    /// steps of now.
    pub fn wrong_code(&self) -> String {
        let now = current_step();
        let near: Vec<String> = (now.saturating_sub(5)..=now + 5)
            .map(|s| code_for_step(&self.secret, s))
            .collect();
        (0..1_000_000u64)
            .map(|n| format!("{:06}", (n * 7919 + 123_456) % 1_000_000))
            .find(|c| !near.contains(c))
            .expect("some wrong code exists")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&sha1(&vec![b'a'; 1_000_000])),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn hmac_sha1_rfc2202() {
        assert_eq!(
            hex(&hmac_sha1(&[0x0b; 20], b"Hi There")),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
        assert_eq!(
            hex(&hmac_sha1(b"Jefe", b"what do ya want for nothing?")),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
        assert_eq!(
            hex(&hmac_sha1(
                &[0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "aa4ae5e15272d00e95705637ce8a3b55ed402112"
        );
    }

    #[test]
    fn base32_roundtrip() {
        let secret = base32_encode(b"12345678901234567890");
        assert_eq!(secret, "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(
            base32_decode(&secret).unwrap(),
            b"12345678901234567890".to_vec()
        );
        assert_eq!(
            base32_decode("gezd gnbv").unwrap(),
            base32_decode("GEZDGNBV").unwrap()
        );
        assert_eq!(base32_decode("MZXW6YQ=").unwrap(), b"foob".to_vec());
        assert!(base32_decode("not base32!").is_none());
        for len in 0..25 {
            let data: Vec<u8> = (0..len as u8).map(|b| b.wrapping_mul(37)).collect();
            assert_eq!(base32_decode(&base32_encode(&data)).unwrap(), data);
        }
    }

    #[test]
    fn totp_rfc6238_sha1_vectors() {
        let key = b"12345678901234567890";
        for (t, code) in [
            (59u64, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(hotp(key, step_at(t), 8), code, "T={t}");
            assert_eq!(hotp(key, step_at(t), 6), code[2..], "T={t} (6 digits)");
        }
        let secret = base32_encode(key);
        assert_eq!(code_at(&secret, 59), "287082");
    }

    #[test]
    fn otpauth_secret_extraction() {
        let text = "Secret: GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\n\
                    URL: otpauth://totp/W0rkTree:erin?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=W0rkTree\n";
        assert_eq!(
            secret_from_otpauth(text).as_deref(),
            Some("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ")
        );
        assert_eq!(secret_from_otpauth("no url here"), None);
    }

    #[test]
    fn totp_helper_never_reuses_a_step() {
        let mut t = Totp::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        let now = current_step();
        t.mark_used(now);
        let c = t.next_code();
        assert_eq!(c.len(), 6);
        assert!(t.last_step.unwrap() > now);
        let wrong = t.wrong_code();
        assert_ne!(wrong, code_now(&t.secret));
    }
}
