use zeroize::Zeroize;

use super::MASK;

/// Collects the secret values used by one send and scrubs them from anything
/// that leaves Rust: history, request echo, response text returned to an agent.
/// Each secret is also registered in the encodings it commonly takes on the wire.
#[derive(Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} patterns)", self.values.len())
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        self.values.iter_mut().for_each(Zeroize::zeroize);
    }
}

/// Variants shorter than this are skipped, they would mask ordinary text.
const MIN_VARIANT_LEN: usize = 4;
/// A secret shorter than this is a placeholder (`0`, `-`), not a credential: masking it would
/// blank every matching character in every response and make the body unreadable.
const MIN_SECRET_LEN: usize = 4;

impl Redactor {
    pub fn add(&mut self, secret: &str) {
        if secret.trim().chars().count() < MIN_SECRET_LEN {
            return;
        }
        self.insert(secret.to_owned());
        for v in variants(secret) {
            if v.len() >= MIN_VARIANT_LEN {
                self.insert(v);
            }
        }
        // Longest first, so a secret that contains another is masked whole.
        self.values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    }

    fn insert(&mut self, v: String) {
        if !v.is_empty() && !self.values.contains(&v) {
            self.values.push(v);
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        self.values
            .iter()
            .fold(text.to_owned(), |acc, v| acc.replace(v.as_str(), MASK))
    }

    /// Byte-level scrub, for bodies of any content kind.
    pub fn scrub_bytes(&self, data: &[u8]) -> Vec<u8> {
        let mut out = data.to_vec();
        for v in &self.values {
            out = replace_bytes(&out, v.as_bytes(), MASK.as_bytes());
        }
        out
    }
}

fn replace_bytes(data: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() || data.len() < needle.len() {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

fn variants(secret: &str) -> Vec<String> {
    let mut out = Vec::new();
    let trimmed = secret.trim();
    out.push(trimmed.to_owned());
    for s in [secret, trimmed] {
        let json = serde_json::to_string(s).unwrap_or_default();
        if json.len() >= 2 {
            let inner = json[1..json.len() - 1].to_owned();
            out.push(inner.replace('/', "\\/"));
            out.push(inner);
        }
        out.push(url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>());
        let pct = percent(s);
        out.push(pct.to_ascii_lowercase_hex());
        out.push(pct);
        for urlsafe in [false, true] {
            for pad in [false, true] {
                out.push(base64(s.as_bytes(), urlsafe, pad));
            }
        }
    }
    out
}

trait LowerHex {
    fn to_ascii_lowercase_hex(&self) -> String;
}

impl LowerHex for String {
    /// Lowercases only the hex digits of `%XX` escapes.
    fn to_ascii_lowercase_hex(&self) -> String {
        let b = self.as_bytes();
        let mut out = String::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                out.push('%');
                out.push((b[i + 1] as char).to_ascii_lowercase());
                out.push((b[i + 2] as char).to_ascii_lowercase());
                i += 3;
            } else {
                out.push(b[i] as char);
                i += 1;
            }
        }
        out
    }
}

fn percent(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn base64(input: &[u8], urlsafe: bool, pad: bool) -> String {
    let alphabet: &[u8; 64] = if urlsafe {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    };
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(alphabet[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_every_occurrence() {
        let mut r = Redactor::default();
        r.add("abc123");
        assert_eq!(r.scrub("Bearer abc123 and abc123"), "Bearer •••• and ••••");
    }

    #[test]
    fn longest_secret_wins_when_one_contains_another() {
        let mut r = Redactor::default();
        r.add("keys");
        r.add("keys-long");
        assert_eq!(r.scrub("x keys-long y"), "x •••• y");
    }

    #[test]
    fn empty_secret_is_ignored() {
        let mut r = Redactor::default();
        r.add("");
        assert_eq!(r.scrub("unchanged"), "unchanged");
    }

    #[test]
    fn scrub_bytes_masks_inside_binary_data() {
        let mut r = Redactor::default();
        r.add("sk-live-123");
        let out = r.scrub_bytes(b"\x00\x01sk-live-123\x02");
        assert_eq!(
            out,
            b"\x00\x01\xE2\x80\xA2\xE2\x80\xA2\xE2\x80\xA2\xE2\x80\xA2\x02"
        );
    }

    #[test]
    fn registers_encoded_variants() {
        let secret = " p@ss w/ord\"é+\n";
        let mut r = Redactor::default();
        r.add(secret);
        let t = secret.trim();
        let pct = percent(t);
        for form in [
            t.to_owned(),
            serde_json::to_string(secret)
                .unwrap()
                .trim_matches('"')
                .to_owned(),
            url::form_urlencoded::byte_serialize(t.as_bytes()).collect::<String>(),
            pct.clone(),
            pct.to_ascii_lowercase_hex(),
            base64(t.as_bytes(), false, true),
            base64(t.as_bytes(), true, false),
        ] {
            let out = r.scrub(&format!("a {form} b"));
            assert_eq!(out, "a •••• b", "form {form}");
        }
    }

    #[test]
    fn placeholder_secrets_do_not_mask_ordinary_text() {
        let mut r = Redactor::default();
        for s in ["0", "-", " 0 ", "ab"] {
            r.add(s);
        }
        assert_eq!(r.scrub("total 39652.67 in 2026"), "total 39652.67 in 2026");
    }

    #[test]
    fn debug_never_prints_values() {
        let mut r = Redactor::default();
        r.add("topsecret");
        assert!(!format!("{r:?}").contains("topsecret"));
    }
}
