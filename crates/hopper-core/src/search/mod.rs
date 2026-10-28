//! One search over spec operations and saved requests, shared by the panel (IPC)
//! and the MCP `hopper_search_routes` tool.
//!
//! Everything searchable is lowered and word-split once, in `set_spec` /
//! `set_saved`; a query only compares tokens against those precomputed fields.
//! Tokens are ANDed. Per token the best field wins: exact word > word prefix >
//! substring > fuzzy (nucleo), each scaled by the field's weight.
//!
//! An operation that a saved request points at (`source`) is returned once, as
//! the saved hit; the operation hit never appears, so `savedRequestId` on
//! operation hits is always absent from this module's output.

use std::collections::{BTreeMap, HashMap, HashSet};

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::{Deserialize, Serialize};

use crate::format::Method;
use crate::openapi::{Operation, SpecIndex};
use crate::policy::AgentAccess;

#[cfg(test)]
mod tests;

const DEFAULT_LIMIT: usize = 50;
const LRU_CAP: usize = 50;
const DESC_CAP: usize = 400;

const SAVED_BOOST: f64 = 30.0;
const RECENT_BOOST: f64 = 25.0;
const DEPRECATED_PENALTY: f64 = 500.0;

const W_PATH: f64 = 1.0;
const W_ID: f64 = 0.9;
const W_TAG: f64 = 0.8;
const W_LABEL: f64 = 0.6;
const W_METHOD: f64 = 0.5;

#[derive(Debug, Clone)]
pub struct SavedItem {
    pub request_id: String,
    pub name: String,
    pub method: Method,
    /// Scheme, host and a leading `{{baseUrl}}` are stripped (again) on intake.
    pub url_template: String,
    pub collection: String,
    pub tags: Vec<String>,
    /// `(spec, operation key)` the request was created from.
    pub source: Option<(String, String)>,
    pub agent_access: AgentAccess,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    #[serde(rename = "query")]
    pub text: String,
    pub collection: Option<String>,
    pub tag: Option<String>,
    pub method: Option<Method>,
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Query {
            text: String::new(),
            collection: None,
            tag: None,
            method: None,
            limit: DEFAULT_LIMIT,
        }
    }
}

impl Query {
    pub fn new(text: impl Into<String>) -> Self {
        Query {
            text: text.into(),
            ..Query::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Hit {
    Saved {
        request_id: String,
        name: String,
        method: Method,
        path: String,
        collection: String,
        matches: Vec<u32>,
        score: f64,
        agent_access: AgentAccess,
    },
    Operation {
        collection: String,
        spec: String,
        key: String,
        method: Method,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        tags: Vec<String>,
        deprecated: bool,
        matches: Vec<u32>,
        score: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        saved_request_id: Option<String>,
    },
}

impl Hit {
    pub fn path(&self) -> &str {
        match self {
            Hit::Saved { path, .. } | Hit::Operation { path, .. } => path,
        }
    }

    pub fn score(&self) -> f64 {
        match self {
            Hit::Saved { score, .. } | Hit::Operation { score, .. } => *score,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MethodCount {
    pub method: Method,
    pub count: usize,
}

/// The TS `SearchResult`: facets are `{name, count}` / `{method, count}` objects.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub total: usize,
    pub tags: Vec<TagCount>,
    pub methods: Vec<MethodCount>,
}

// ---------------------------------------------------------------- documents

#[derive(Debug, Clone)]
struct Field {
    text: String,
    lower: String,
    words: Vec<String>,
}

impl Field {
    fn new(text: &str) -> Self {
        Field {
            text: text.to_owned(),
            lower: text.to_lowercase(),
            words: split_words(text),
        }
    }

    fn path(path: &str) -> Self {
        let mut words = Vec::new();
        for seg in path.split('/') {
            let seg = seg.trim_matches(|c| c == '{' || c == '}');
            if seg.is_empty() {
                continue;
            }
            push_unique(&mut words, seg.to_lowercase());
            for w in split_words(seg) {
                push_unique(&mut words, w);
            }
        }
        Field {
            text: path.to_owned(),
            lower: path.to_lowercase(),
            words,
        }
    }
}

#[derive(Debug, Clone)]
enum Kind {
    Op {
        spec: String,
        key: String,
        summary: Option<String>,
        deprecated: bool,
    },
    Saved {
        request_id: String,
        name: String,
        access: AgentAccess,
    },
}

#[derive(Debug, Clone)]
struct Doc {
    collection: String,
    kind: Kind,
    method: Method,
    path: Field,
    op_id: Option<Field>,
    tags: Vec<Field>,
    label: Option<Field>,
    desc: Option<String>,
}

impl Doc {
    fn from_operation(collection: &str, spec: &str, op: &Operation) -> Doc {
        let desc = op.description.as_deref().map(|d| {
            let cut: String = d.chars().take(DESC_CAP).collect();
            cut.to_lowercase()
        });
        Doc {
            collection: collection.to_owned(),
            kind: Kind::Op {
                spec: spec.to_owned(),
                key: op.key.clone(),
                summary: op.summary.clone(),
                deprecated: op.deprecated,
            },
            method: op.method,
            path: Field::path(&op.path),
            op_id: op.operation_id.as_deref().map(Field::new),
            tags: op.tags.iter().map(|t| Field::new(t)).collect(),
            label: op.summary.as_deref().map(Field::new),
            desc,
        }
    }

    fn from_saved(item: &SavedItem, source: Option<&Doc>) -> Doc {
        let path = path_of(&item.url_template);
        let tags: Vec<Field> = if item.tags.is_empty() {
            source.map(|s| s.tags.clone()).unwrap_or_default()
        } else {
            item.tags.iter().map(|t| Field::new(t)).collect()
        };
        Doc {
            collection: item.collection.clone(),
            kind: Kind::Saved {
                request_id: item.request_id.clone(),
                name: item.name.clone(),
                access: item.agent_access,
            },
            method: item.method,
            path: Field::path(&path),
            op_id: source.and_then(|s| s.op_id.clone()),
            tags,
            label: Some(Field::new(&item.name)),
            desc: source.and_then(|s| match &s.kind {
                Kind::Op { summary, .. } => summary.as_deref().map(str::to_lowercase),
                Kind::Saved { .. } => None,
            }),
        }
    }

    fn is_saved(&self) -> bool {
        matches!(self.kind, Kind::Saved { .. })
    }

    fn deprecated(&self) -> bool {
        matches!(
            self.kind,
            Kind::Op {
                deprecated: true,
                ..
            }
        )
    }
}

#[derive(Default)]
struct SpecDocs {
    docs: Vec<Doc>,
    by_key: HashMap<String, usize>,
}

// -------------------------------------------------------------------- index

#[derive(Default)]
pub struct SearchIndex {
    specs: BTreeMap<(String, String), SpecDocs>,
    saved_items: Vec<SavedItem>,
    saved_docs: Vec<Doc>,
    /// `(collection, spec, key)` of operations that have a saved request.
    covered: HashSet<(String, String, String)>,
    /// Most recent first.
    recent: Vec<String>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_spec(&mut self, collection: &str, spec: &str, index: &SpecIndex) {
        let mut sd = SpecDocs::default();
        for op in &index.operations {
            sd.by_key.insert(op.key.clone(), sd.docs.len());
            sd.docs.push(Doc::from_operation(collection, spec, op));
        }
        self.specs
            .insert((collection.to_owned(), spec.to_owned()), sd);
        self.rebuild_saved();
    }

    pub fn remove_spec(&mut self, collection: &str, spec: &str) {
        self.specs.remove(&(collection.to_owned(), spec.to_owned()));
        self.rebuild_saved();
    }

    pub fn set_saved(&mut self, items: Vec<SavedItem>) {
        self.saved_items = items;
        self.rebuild_saved();
    }

    pub fn record_use(&mut self, request_id: &str) {
        self.recent.retain(|r| r != request_id);
        self.recent.insert(0, request_id.to_owned());
        self.recent.truncate(LRU_CAP);
    }

    fn rebuild_saved(&mut self) {
        self.covered.clear();
        let mut docs = Vec::with_capacity(self.saved_items.len());
        for item in &self.saved_items {
            let source = item.source.as_ref().and_then(|(spec, key)| {
                let sd = self.specs.get(&(item.collection.clone(), spec.clone()))?;
                let doc = sd.docs.get(*sd.by_key.get(key)?)?;
                self.covered
                    .insert((item.collection.clone(), spec.clone(), key.clone()));
                Some(doc)
            });
            docs.push(Doc::from_saved(item, source));
        }
        self.saved_docs = docs;
    }

    fn is_covered(&self, doc: &Doc) -> bool {
        match &doc.kind {
            Kind::Op { spec, key, .. } => {
                !self.covered.is_empty()
                    && self
                        .covered
                        .contains(&(doc.collection.clone(), spec.clone(), key.clone()))
            }
            Kind::Saved { .. } => false,
        }
    }

    fn recent_pos(&self, doc: &Doc) -> Option<usize> {
        match &doc.kind {
            Kind::Saved { request_id, .. } => self.recent.iter().position(|r| r == request_id),
            Kind::Op { .. } => None,
        }
    }

    pub fn search(&self, q: &Query) -> SearchResult {
        let limit = if q.limit == 0 { DEFAULT_LIMIT } else { q.limit };
        let parsed = Parsed::new(&q.text);
        let scoring = parsed.is_scoring();
        let mut fz = Fuzzy::new();
        let tag_exact = q.tag.as_deref().map(str::to_lowercase);

        let mut tag_counts: HashMap<&str, usize> = HashMap::new();
        let mut method_counts = [0usize; 7];
        let mut cands: Vec<(f64, &Doc)> = Vec::new();

        let all = self
            .saved_docs
            .iter()
            .chain(self.specs.values().flat_map(|s| s.docs.iter()));
        for doc in all {
            if q.collection.as_deref().is_some_and(|c| c != doc.collection) {
                continue;
            }
            if self.is_covered(doc) {
                continue;
            }
            let Some(mut score) = parsed.score(doc, &mut fz) else {
                continue;
            };
            let tag_ok = tag_exact
                .as_deref()
                .is_none_or(|t| doc.tags.iter().any(|f| f.lower == t))
                && parsed
                    .tags
                    .iter()
                    .all(|t| doc.tags.iter().any(|f| f.lower.contains(t.as_str())));
            let method_ok = q.method.is_none_or(|m| m == doc.method)
                && parsed.method.is_none_or(|m| m == doc.method);

            if method_ok {
                let mut seen: Vec<&str> = Vec::new();
                for t in &doc.tags {
                    if !seen.contains(&t.text.as_str()) {
                        seen.push(&t.text);
                        *tag_counts.entry(&t.text).or_default() += 1;
                    }
                }
            }
            if tag_ok {
                method_counts[method_idx(doc.method)] += 1;
            }
            if !(tag_ok && method_ok) {
                continue;
            }
            if scoring {
                if doc.is_saved() {
                    score += SAVED_BOOST;
                }
                if let Some(pos) = self.recent_pos(doc) {
                    score += RECENT_BOOST * (1.0 - pos as f64 / LRU_CAP as f64);
                }
                if doc.deprecated() {
                    score -= DEPRECATED_PENALTY;
                }
                score -= doc.path.text.len() as f64 * 0.01;
            }
            cands.push((score, doc));
        }

        if scoring {
            cands.sort_by(|a, b| {
                b.0.total_cmp(&a.0)
                    .then_with(|| tie_key(a.1).cmp(&tie_key(b.1)))
            });
        } else {
            cands.sort_by_cached_key(|c| self.browse_key(c.1));
        }

        let total = cands.len();
        let hits = cands
            .iter()
            .take(limit)
            .map(|(score, doc)| self.to_hit(doc, *score, &parsed, &mut fz))
            .collect();

        let mut tags: Vec<TagCount> = tag_counts
            .into_iter()
            .map(|(name, count)| TagCount {
                name: name.to_owned(),
                count,
            })
            .collect();
        tags.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        let methods = METHODS
            .iter()
            .enumerate()
            .filter(|(i, _)| method_counts[*i] > 0)
            .map(|(i, m)| MethodCount {
                method: *m,
                count: method_counts[i],
            })
            .collect::<Vec<_>>();
        let mut methods = methods;
        methods.sort_by_key(|m| std::cmp::Reverse(m.count));

        SearchResult {
            hits,
            total,
            tags,
            methods,
        }
    }

    /// Saved first (recently used, then by name), then operations by path.
    fn browse_key(&self, doc: &Doc) -> (u8, usize, String, usize) {
        match &doc.kind {
            Kind::Saved { name, .. } => (
                0,
                self.recent_pos(doc).unwrap_or(usize::MAX),
                name.to_lowercase(),
                method_idx(doc.method),
            ),
            Kind::Op { deprecated, .. } => (
                if *deprecated { 2 } else { 1 },
                0,
                doc.path.lower.clone(),
                method_idx(doc.method),
            ),
        }
    }

    fn to_hit(&self, doc: &Doc, score: f64, parsed: &Parsed, fz: &mut Fuzzy) -> Hit {
        let matches = parsed.path_matches(&doc.path.text, fz);
        let path = doc.path.text.clone();
        let collection = doc.collection.clone();
        match &doc.kind {
            Kind::Saved {
                request_id,
                name,
                access,
            } => Hit::Saved {
                request_id: request_id.clone(),
                name: name.clone(),
                method: doc.method,
                path,
                collection,
                matches,
                score,
                agent_access: *access,
            },
            Kind::Op {
                spec,
                key,
                summary,
                deprecated,
            } => Hit::Operation {
                collection,
                spec: spec.clone(),
                key: key.clone(),
                method: doc.method,
                path,
                summary: summary.clone(),
                tags: doc.tags.iter().map(|t| t.text.clone()).collect(),
                deprecated: *deprecated,
                matches,
                score,
                saved_request_id: None,
            },
        }
    }
}

fn tie_key(doc: &Doc) -> (usize, &str, usize) {
    (doc.path.text.len(), &doc.path.text, method_idx(doc.method))
}

const METHODS: [Method; 7] = [
    Method::Get,
    Method::Post,
    Method::Put,
    Method::Patch,
    Method::Delete,
    Method::Head,
    Method::Options,
];

fn method_idx(m: Method) -> usize {
    METHODS.iter().position(|x| *x == m).unwrap_or(0)
}

fn method_word(w: &str) -> Option<Method> {
    Some(match w {
        "get" => Method::Get,
        "post" => Method::Post,
        "put" => Method::Put,
        "patch" => Method::Patch,
        "delete" => Method::Delete,
        "head" => Method::Head,
        "options" => Method::Options,
        _ => return None,
    })
}

fn method_lower(m: Method) -> &'static str {
    match m {
        Method::Get => "get",
        Method::Post => "post",
        Method::Put => "put",
        Method::Patch => "patch",
        Method::Delete => "delete",
        Method::Head => "head",
        Method::Options => "options",
    }
}

// -------------------------------------------------------------------- query

struct Tok {
    lower: String,
    pattern: Pattern,
}

impl Tok {
    fn new(text: &str) -> Self {
        Tok {
            lower: text.to_lowercase(),
            pattern: Pattern::new(
                text,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
            ),
        }
    }
}

struct Parsed {
    words: Vec<Tok>,
    ids: Vec<Tok>,
    paths: Vec<String>,
    tags: Vec<String>,
    method: Option<Method>,
}

impl Parsed {
    fn new(text: &str) -> Self {
        let mut p = Parsed {
            words: Vec::new(),
            ids: Vec::new(),
            paths: Vec::new(),
            tags: Vec::new(),
            method: None,
        };
        for (i, raw) in text.split_whitespace().enumerate() {
            let lower = raw.to_lowercase();
            if let Some(v) = lower.strip_prefix("tag:") {
                if !v.is_empty() {
                    p.tags.push(v.to_owned());
                }
            } else if let Some(v) = lower.strip_prefix("path:") {
                if !v.is_empty() {
                    p.paths.push(v.to_owned());
                }
            } else if let Some(v) = raw.strip_prefix('#') {
                if !v.is_empty() {
                    p.ids.push(Tok::new(v));
                }
            } else if i == 0
                && let Some(m) = method_word(&lower)
            {
                p.method = Some(m);
            } else {
                p.words.push(Tok::new(raw));
            }
        }
        p
    }

    fn is_scoring(&self) -> bool {
        !(self.words.is_empty() && self.ids.is_empty() && self.paths.is_empty())
    }

    /// `None` when any token fails to match; the score is 0 with no tokens.
    fn score(&self, doc: &Doc, fz: &mut Fuzzy) -> Option<f64> {
        let mut total = 0.0;
        for t in &self.words {
            total += word_score(t, doc, fz)?;
        }
        for t in &self.ids {
            total += id_score(t, doc, fz)?;
        }
        for p in &self.paths {
            if !doc.path.lower.contains(p.as_str()) {
                return None;
            }
            total += if doc.path.lower.starts_with(p.as_str()) {
                200.0
            } else {
                150.0
            };
        }
        Some(total)
    }

    /// Char indices of the displayed path that the tokens matched.
    fn path_matches(&self, path: &str, fz: &mut Fuzzy) -> Vec<u32> {
        let chars: Vec<char> = path.chars().map(|c| c.to_ascii_lowercase()).collect();
        let mut out: Vec<u32> = Vec::new();
        for t in &self.words {
            let needle: Vec<char> = t.lower.chars().collect();
            if let Some(start) = find_sub(&chars, &needle) {
                out.extend((start..start + needle.len()).map(|i| i as u32));
            } else {
                fz.indices(&t.pattern, path, &mut out);
            }
        }
        for p in &self.paths {
            let needle: Vec<char> = p.chars().collect();
            if let Some(start) = find_sub(&chars, &needle) {
                out.extend((start..start + needle.len()).map(|i| i as u32));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// First occurrence at a word boundary, else the first occurrence.
fn find_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let mut first = None;
    for i in 0..=hay.len() - needle.len() {
        if &hay[i..i + needle.len()] == needle {
            if i == 0 || !hay[i - 1].is_alphanumeric() {
                return Some(i);
            }
            first.get_or_insert(i);
        }
    }
    first
}

fn word_score(t: &Tok, doc: &Doc, fz: &mut Fuzzy) -> Option<f64> {
    let tok = t.lower.as_str();
    let mut best = field_score(tok, &doc.path) * W_PATH;
    if let Some(f) = &doc.op_id {
        best = best.max(field_score(tok, f) * W_ID);
    }
    for f in &doc.tags {
        best = best.max(field_score(tok, f) * W_TAG);
    }
    if let Some(f) = &doc.label {
        best = best.max(field_score(tok, f) * W_LABEL);
    }
    if method_lower(doc.method) == tok {
        best = best.max(100.0 * W_METHOD);
    }
    if best < 20.0 && doc.desc.as_deref().is_some_and(|d| d.contains(tok)) {
        best = best.max(8.0);
    }
    if best < 20.0 && tok.chars().count() >= 2 {
        let mut f = fz.score(&t.pattern, &doc.path.text, tok) * W_PATH;
        if let Some(id) = &doc.op_id {
            f = f.max(fz.score(&t.pattern, &id.text, tok) * W_ID);
        }
        for tag in &doc.tags {
            f = f.max(fz.score(&t.pattern, &tag.text, tok) * W_TAG);
        }
        if let Some(l) = &doc.label {
            f = f.max(fz.score(&t.pattern, &l.text, tok) * W_LABEL);
        }
        best = best.max(f);
    }
    (best > 0.0).then_some(best)
}

fn id_score(t: &Tok, doc: &Doc, fz: &mut Fuzzy) -> Option<f64> {
    let id = doc.op_id.as_ref()?;
    let tok = t.lower.as_str();
    if id.lower == tok {
        Some(200.0)
    } else if id.lower.starts_with(tok) {
        Some(120.0)
    } else if id.lower.contains(tok) {
        Some(80.0)
    } else if tok.chars().count() >= 3 {
        let s = fz.score(&t.pattern, &id.text, tok);
        (s > 0.0).then_some(s * 1.5)
    } else {
        None
    }
}

fn field_score(tok: &str, f: &Field) -> f64 {
    let mut best = 0.0;
    for w in &f.words {
        if w == tok {
            return 100.0;
        }
        if w.starts_with(tok) {
            best = 70.0;
        }
    }
    if best == 0.0 && f.lower.contains(tok) {
        best = 45.0;
    }
    best
}

/// Reused matcher and buffer; fuzzy scores land in 15..=25.
struct Fuzzy {
    matcher: Matcher,
    buf: Vec<char>,
}

impl Fuzzy {
    fn new() -> Self {
        Fuzzy {
            matcher: Matcher::new(Config::DEFAULT),
            buf: Vec::new(),
        }
    }

    fn score(&mut self, pat: &Pattern, text: &str, tok: &str) -> f64 {
        let hay = Utf32Str::new(text, &mut self.buf);
        match pat.score(hay, &mut self.matcher) {
            Some(s) => {
                let per_char = f64::from(s) / (tok.chars().count() as f64 * 24.0);
                15.0 + per_char.min(1.0) * 10.0
            }
            None => 0.0,
        }
    }

    fn indices(&mut self, pat: &Pattern, text: &str, out: &mut Vec<u32>) {
        let hay = Utf32Str::new(text, &mut self.buf);
        pat.indices(hay, &mut self.matcher, out);
    }
}

// ------------------------------------------------------------------ helpers

fn push_unique(v: &mut Vec<String>, s: String) {
    if !s.is_empty() && !v.contains(&s) {
        v.push(s);
    }
}

/// Lowercase words of `s`: each alphanumeric chunk whole, plus its camelCase /
/// PascalCase pieces (`UserProfile` gives `userprofile`, `user`, `profile`).
fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in s.split(|c: char| !c.is_alphanumeric()) {
        if chunk.is_empty() {
            continue;
        }
        push_unique(&mut out, chunk.to_lowercase());
        for piece in camel_pieces(chunk) {
            push_unique(&mut out, piece);
        }
    }
    out
}

fn camel_pieces(chunk: &str) -> Vec<String> {
    let chars: Vec<char> = chunk.chars().collect();
    let mut pieces = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (prev, cur) = (chars[i - 1], chars[i]);
        let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
        let boundary = ((prev.is_lowercase() || prev.is_ascii_digit()) && cur.is_uppercase())
            || (prev.is_uppercase() && cur.is_uppercase() && next_lower);
        if boundary {
            pieces.push(chars[start..i].iter().collect::<String>().to_lowercase());
            start = i;
        }
    }
    if start > 0 {
        pieces.push(chars[start..].iter().collect::<String>().to_lowercase());
    }
    pieces
}

/// Path part of a URL template: no scheme/host, no leading `{{var}}`, no query.
fn path_of(url: &str) -> String {
    let mut rest = url.trim();
    if rest.starts_with("{{")
        && let Some(end) = rest.find("}}")
    {
        rest = &rest[end + 2..];
    } else if let Some(i) = rest.find("://") {
        let after = &rest[i + 3..];
        rest = after.find('/').map_or("", |s| &after[s..]);
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    if rest.starts_with('/') {
        rest.to_owned()
    } else {
        format!("/{rest}")
    }
}
