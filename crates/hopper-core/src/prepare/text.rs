//! Segmented strings and the small encoders `prepare` needs. A `Text` remembers
//! which parts came from secrets, so masked output is built per segment
//! instead of scrubbed afterwards.

use crate::secrets::MASK;

#[derive(Clone, PartialEq)]
pub(super) enum Seg {
    Plain(String),
    Secret(String),
}

impl std::fmt::Debug for Seg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Seg::Plain(v) => write!(f, "Plain({v:?})"),
            Seg::Secret(_) => write!(f, "Secret({MASK})"),
        }
    }
}

impl Drop for Seg {
    fn drop(&mut self) {
        if let Seg::Secret(v) = self {
            zeroize::Zeroize::zeroize(v);
        }
    }
}

#[derive(Clone, Default, PartialEq)]
pub(super) struct Text(Vec<Seg>);

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Text{:?}", self.0)
    }
}

impl Text {
    pub fn plain(s: &str) -> Self {
        let mut t = Self::default();
        t.push_plain(s);
        t
    }

    pub fn push_plain(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if let Some(Seg::Plain(last)) = self.0.last_mut() {
            last.push_str(s);
        } else {
            self.0.push(Seg::Plain(s.to_owned()));
        }
    }

    pub fn push_secret(&mut self, s: &str) {
        self.0.push(Seg::Secret(s.to_owned()));
    }

    pub fn extend(&mut self, other: &Text) {
        for seg in &other.0 {
            match seg {
                Seg::Plain(s) => self.push_plain(s),
                Seg::Secret(s) => self.push_secret(s),
            }
        }
    }

    pub fn real(&self) -> String {
        self.0
            .iter()
            .map(|s| match s {
                Seg::Plain(v) | Seg::Secret(v) => v.as_str(),
            })
            .collect()
    }

    pub fn masked(&self) -> String {
        self.0
            .iter()
            .map(|s| match s {
                Seg::Plain(v) => v.as_str(),
                Seg::Secret(_) => MASK,
            })
            .collect()
    }

    pub fn has_secret(&self) -> bool {
        self.0.iter().any(|s| matches!(s, Seg::Secret(_)))
    }

    /// Plain segments only: what is safe to inspect structurally.
    pub fn plain_contains(&self, c: char) -> bool {
        self.0
            .iter()
            .any(|s| matches!(s, Seg::Plain(v) if v.contains(c)))
    }

    pub fn ends_with_plain(&self, chars: &[char]) -> bool {
        matches!(self.0.last(), Some(Seg::Plain(v)) if v.ends_with(chars))
    }

    /// Splits at the first `#` in a plain segment: (before, from the `#` on).
    pub fn split_fragment(&self) -> (Text, Text) {
        let mut head = Text::default();
        let mut tail = Text::default();
        let mut in_tail = false;
        for seg in &self.0 {
            match seg {
                Seg::Plain(v) if !in_tail && v.contains('#') => {
                    let i = v.find('#').expect("checked");
                    head.push_plain(&v[..i]);
                    tail.push_plain(&v[i..]);
                    in_tail = true;
                }
                Seg::Plain(v) if in_tail => tail.push_plain(v),
                Seg::Plain(v) => head.push_plain(v),
                Seg::Secret(v) if in_tail => tail.push_secret(v),
                Seg::Secret(v) => head.push_secret(v),
            }
        }
        (head, tail)
    }

    pub fn map(&self, f: impl Fn(&str) -> String) -> Text {
        Text(
            self.0
                .iter()
                .map(|s| match s {
                    Seg::Plain(v) => Seg::Plain(f(v)),
                    Seg::Secret(v) => Seg::Secret(f(v)),
                })
                .collect(),
        )
    }
}

pub(super) enum Part<'a> {
    Lit(&'a str),
    Var(&'a str),
}

fn is_var_name(s: &str) -> bool {
    let body = s.strip_prefix('$').unwrap_or(s);
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Splits `{{name}}` references out of a template. Anything that does not look
/// like a variable name (JSON like `{{"a":1}}`, `{{ two words }}`) stays literal.
pub(super) fn parse_template(s: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = s;
    while let Some(pos) = rest.find("{{") {
        let after = &rest[pos + 2..];
        match after.find("}}") {
            Some(end) if is_var_name(after[..end].trim()) => {
                if pos > 0 {
                    parts.push(Part::Lit(&rest[..pos]));
                }
                parts.push(Part::Var(after[..end].trim()));
                rest = &after[end + 2..];
            }
            _ => {
                parts.push(Part::Lit(&rest[..pos + 1]));
                rest = &rest[pos + 1..];
            }
        }
    }
    if !rest.is_empty() {
        parts.push(Part::Lit(rest));
    }
    parts
}

pub(super) fn is_param_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
pub(super) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
