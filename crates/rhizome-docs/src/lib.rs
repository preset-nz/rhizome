//! rhizome-docs: markdown pages from a rhizome [`Schema`].
//!
//! One page per node type the app declared, named `<type>.md`, plus a `README.md` index. A
//! page has a generated block between two markers and free prose around it:
//!
//! ```markdown
//! <!-- generated:begin — rhizome-docs rewrites this block; write outside it -->
//! (the type's summary and a table of its values)
//! <!-- generated:end -->
//! ```
//!
//! Regenerating rewrites only what lies between the markers and the front-matter keys the
//! generator owns (`title`, `node`, `group`, `summary`); every other byte is kept. A new page
//! gets front matter, a heading, the block and an empty "What it's for" section for a person
//! to write. The index is generated whole.
//!
//! Every type in the schema gets a page. The core's own types (root, category, group) never
//! appear in a schema, so there is nothing to skip. A type with no label is titled by its
//! name.
//!
//! The crate knows no app: it reads only the words an app put on its types and values
//! (`label`, `summary`, `group`, `description`, `unit`, `names`).
//!
//! ```
//! use rhizome_core::{NodeType, Origin, Registry};
//!
//! let registry = Registry::builder()
//!     .category("things", Origin::Loaded)
//!     .node(
//!         NodeType::new("widget")
//!             .label("Widget")
//!             .summary("A made-up node.")
//!             .float("size", 0.0..=10.0, 1.0)
//!             .described("Size", "How big it is.")
//!             .unit("cm"),
//!     )
//!     .build()
//!     .unwrap();
//! let pages = rhizome_docs::pages(&registry.schema(), "Example");
//! let widget = pages.iter().find(|p| p.file_name == "widget.md").unwrap();
//! assert!(widget.content.contains("| Size | 0 cm to 10 cm | 1 cm | How big it is. |"));
//! // a page someone wrote prose around keeps it
//! let edited = widget.content.replace("## What it's for\n", "## What it's for\n\nMine.\n");
//! assert!(rhizome_docs::update(Some(&edited), widget).contains("Mine."));
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use rhizome_core::{Schema, TypeSchema, ValueKind, ValueSchema};
use serde_json::Value as Json;

/// The start of the begin marker. The rest of its line, up to `-->`, is free.
pub const BEGIN: &str = "<!-- generated:begin";
/// The end marker.
pub const END: &str = "<!-- generated:end -->";
/// The index page's file name.
pub const INDEX: &str = "README.md";

const BEGIN_LINE: &str =
    "<!-- generated:begin — rhizome-docs rewrites this block; write outside it -->";

/// One markdown file: its name in the docs folder and its full content as generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub file_name: String,
    pub content: String,
}

/// Reads a schema an app dumped: a [`Schema`] as JSON, or any object holding one under
/// `schema` (such as a POM view).
pub fn read_schema(json: &str) -> serde_json::Result<Schema> {
    let mut v: Json = serde_json::from_str(json)?;
    if let Some(inner) = v.get_mut("schema") {
        v = inner.take();
    }
    serde_json::from_value(v)
}

/// Every page as freshly generated, in name order: one per node type, then the index. `app`
/// names the index's title ("<app> nodes"); empty gives "Nodes".
pub fn pages(schema: &Schema, app: &str) -> Vec<Page> {
    let mut pages: Vec<Page> = types(schema).map(page).collect();
    pages.push(index(schema, app));
    pages
}

/// The front-matter keys the generator owns. [`update`] rewrites exactly these.
pub const GENERATED_KEYS: [&str; 4] = ["title", "node", "group", "summary"];

/// The page `generated` merged into what is on disk. With no page yet, the generated page.
/// With one, two places change and nothing else:
///
/// - **The block:** only the text between its markers is replaced; the marker lines are kept.
///   A page with no begin marker gets the block appended at its end; one with a begin marker
///   and no end marker keeps its body unchanged (and [`check`] reports it).
/// - **The front matter:** the keys in [`GENERATED_KEYS`] are rewritten in place, added
///   (after the generated key before them, else first) or removed, as the schema says. Every
///   other key keeps its line, its place and its value byte for byte. A page with no front
///   matter gets the generated one in front of it.
///
/// The index has no block: it is always replaced whole.
pub fn update(existing: Option<&str>, generated: &Page) -> String {
    let Some(existing) = existing else {
        return generated.content.clone();
    };
    update_front_matter(&update_block(existing, generated), generated)
}

fn update_block(existing: &str, generated: &Page) -> String {
    let Some(fresh) = inner(&generated.content) else {
        return generated.content.clone();
    };
    let fresh = &generated.content[fresh];
    match find_block(existing) {
        Block::Found(range) => {
            let mut out = String::with_capacity(existing.len() + fresh.len());
            out.push_str(&existing[..range.start]);
            out.push_str(fresh);
            out.push_str(&existing[range.end..]);
            out
        }
        Block::Broken => existing.to_string(),
        Block::None => {
            let mut out = existing.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(BEGIN_LINE);
            out.push_str(fresh);
            out.push_str(END);
            out.push('\n');
            out
        }
    }
}

fn update_front_matter(existing: &str, generated: &Page) -> String {
    let Some(fresh_span) = front_matter(&generated.content) else {
        return existing.to_string();
    };
    let fresh_text = &generated.content[fresh_span];
    let Some(span) = front_matter(existing) else {
        return format!("---\n{fresh_text}---\n\n{existing}");
    };
    let fresh: BTreeMap<&str, &str> = fresh_text
        .split_inclusive('\n')
        .filter_map(|l| Some((top_key(l)?, l)))
        .collect();
    let owned = |k: &str| GENERATED_KEYS.contains(&k);
    let mut lines: Vec<&str> = Vec::new();
    let mut used: Vec<&str> = Vec::new();
    let mut in_owned = false;
    for line in existing[span.clone()].split_inclusive('\n') {
        match top_key(line) {
            Some(k) if owned(k) => {
                in_owned = true;
                if let Some(l) = fresh.get(k).filter(|_| !used.contains(&k)) {
                    lines.push(l);
                    used.push(k);
                }
            }
            // an owned key's continuation lines go with it
            None if in_owned && line.starts_with([' ', '\t']) => {}
            _ => {
                in_owned = false;
                lines.push(line);
            }
        }
    }
    for (i, k) in GENERATED_KEYS.iter().enumerate() {
        let Some(l) = fresh.get(k).filter(|_| !used.contains(k)) else {
            continue;
        };
        let at = lines
            .iter()
            .rposition(|line| top_key(line).is_some_and(|lk| GENERATED_KEYS[..i].contains(&lk)))
            .map_or(0, |p| p + 1);
        lines.insert(at, l);
        used.push(k);
    }
    let mut out = String::with_capacity(existing.len());
    out.push_str(&existing[..span.start]);
    for l in lines {
        out.push_str(l);
    }
    out.push_str(&existing[span.end..]);
    out
}

/// The text between a leading `---` line and the next `---` line, if the page opens with one.
fn front_matter(s: &str) -> Option<std::ops::Range<usize>> {
    let open = ["---\n", "---\r\n"]
        .iter()
        .find(|o| s.starts_with(**o))?
        .len();
    let mut at = open;
    for line in s[open..].split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Some(open..at);
        }
        at += line.len();
    }
    None
}

/// A front-matter line's top-level key: `summary` in `summary: …`.
fn top_key(line: &str) -> Option<&str> {
    if line.starts_with([' ', '\t', '#', '-']) {
        return None;
    }
    let key = &line[..line.find(':')?];
    (!key.is_empty() && !key.contains(char::is_whitespace)).then_some(key)
}

/// What [`check`] found. Warnings don't fail a check; the rest do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    /// A type has no page yet.
    MissingPage { file: String },
    /// A page's generated block differs from what the schema says now.
    StaleBlock { file: String },
    /// A page has no generated block, or a begin marker without an end.
    NoBlock { file: String },
    /// A page's generated front-matter keys differ from what the schema says now.
    StaleFrontMatter { file: String },
    /// The index differs from what the schema says now.
    StaleIndex,
    /// A page whose type is gone. Reported, never deleted.
    Orphan { file: String },
    /// A type with no summary.
    MissingSummary { node: String },
    /// A value with no description. A warning, so the check can land before the words do.
    MissingDescription { node: String, key: String },
}

impl Finding {
    pub fn is_warning(&self) -> bool {
        matches!(self, Finding::MissingDescription { .. })
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::MissingPage { file } => write!(f, "{file}: no page yet"),
            Finding::StaleBlock { file } => write!(f, "{file}: generated block is out of date"),
            Finding::NoBlock { file } => write!(f, "{file}: no complete generated block"),
            Finding::StaleFrontMatter { file } => write!(f, "{file}: front matter is out of date"),
            Finding::StaleIndex => write!(f, "{INDEX}: out of date"),
            Finding::Orphan { file } => write!(f, "{file}: no node of this name any more"),
            Finding::MissingSummary { node } => write!(f, "{node}: no summary"),
            Finding::MissingDescription { node, key } => {
                write!(f, "{node}.{key}: no description")
            }
        }
    }
}

/// Compares `dir` with what the schema would generate, writing nothing.
pub fn check(dir: &Path, schema: &Schema, app: &str) -> io::Result<Vec<Finding>> {
    let mut found = Vec::new();
    for t in types(schema) {
        if t.summary.is_none() {
            found.push(Finding::MissingSummary {
                node: t.name.clone(),
            });
        }
        for v in t.values.iter().chain(t.bindable.iter().flatten()) {
            if v.description.is_none() {
                found.push(Finding::MissingDescription {
                    node: t.name.clone(),
                    key: v.key.clone(),
                });
            }
        }
    }
    for p in pages(schema, app) {
        let Some(on_disk) = read(&dir.join(&p.file_name))? else {
            found.push(if p.file_name == INDEX {
                Finding::StaleIndex
            } else {
                Finding::MissingPage { file: p.file_name }
            });
            continue;
        };
        if p.file_name == INDEX {
            if on_disk != p.content {
                found.push(Finding::StaleIndex);
            }
        } else {
            if update_front_matter(&on_disk, &p) != on_disk {
                found.push(Finding::StaleFrontMatter {
                    file: p.file_name.clone(),
                });
            }
            if !matches!(find_block(&on_disk), Block::Found(_)) {
                found.push(Finding::NoBlock { file: p.file_name });
            } else if update_block(&on_disk, &p) != on_disk {
                found.push(Finding::StaleBlock { file: p.file_name });
            }
        }
    }
    for file in orphans(dir, schema)? {
        found.push(Finding::Orphan { file });
    }
    Ok(found)
}

/// Writes every page into `dir`, creating it if needed: new pages whole, existing ones only
/// between their markers, the index whole. Returns the files it changed. Orphaned pages are
/// left alone; [`check`] reports them.
pub fn write(dir: &Path, schema: &Schema, app: &str) -> io::Result<Vec<String>> {
    fs::create_dir_all(dir)?;
    let mut changed = Vec::new();
    for p in pages(schema, app) {
        let path = dir.join(&p.file_name);
        let on_disk = read(&path)?;
        let next = if p.file_name == INDEX {
            p.content.clone()
        } else {
            update(on_disk.as_deref(), &p)
        };
        if on_disk.as_deref() != Some(next.as_str()) {
            fs::write(&path, next)?;
            changed.push(p.file_name);
        }
    }
    Ok(changed)
}

/// Markdown files in `dir` that no type in the schema would write, in name order.
pub fn orphans(dir: &Path, schema: &Schema) -> io::Result<Vec<String>> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for e in entries {
        let name = e?.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".md") else {
            continue;
        };
        if name != INDEX && !schema.types.iter().any(|t| t.name == stem) {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

fn read(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn types(schema: &Schema) -> impl Iterator<Item = &TypeSchema> {
    let mut types: Vec<&TypeSchema> = schema.types.iter().collect();
    types.sort_by(|a, b| a.name.cmp(&b.name));
    types.into_iter()
}

enum Block {
    /// The text between the begin marker's `-->` and the end marker.
    Found(std::ops::Range<usize>),
    Broken,
    None,
}

fn find_block(s: &str) -> Block {
    let Some(begin) = s.find(BEGIN) else {
        return Block::None;
    };
    let Some(close) = s[begin..].find("-->") else {
        return Block::Broken;
    };
    let start = begin + close + 3;
    match s[start..].find(END) {
        Some(end) => Block::Found(start..start + end),
        None => Block::Broken,
    }
}

fn inner(s: &str) -> Option<std::ops::Range<usize>> {
    match find_block(s) {
        Block::Found(r) => Some(r),
        _ => None,
    }
}

fn title(t: &TypeSchema) -> &str {
    t.label.as_deref().unwrap_or(&t.name)
}

/// A new page for `t`: front matter, heading, block, and an empty section for prose.
fn page(t: &TypeSchema) -> Page {
    let mut s = String::from("---\n");
    s.push_str(&format!("title: {}\n", yaml(title(t))));
    s.push_str(&format!("node: {}\n", yaml(&t.name)));
    if let Some(g) = &t.group {
        s.push_str(&format!("group: {}\n", yaml(g)));
    }
    if let Some(sum) = &t.summary {
        s.push_str(&format!("summary: {}\n", yaml(sum)));
    }
    s.push_str("---\n\n");
    s.push_str(&format!("# {}\n\n", one_line(title(t))));
    s.push_str(&block(t));
    s.push_str("\n## What it's for\n");
    Page {
        file_name: format!("{}.md", t.name),
        content: s,
    }
}

/// The generated block, markers included, ending in a newline.
fn block(t: &TypeSchema) -> String {
    let mut s = String::from(BEGIN_LINE);
    s.push('\n');
    if let Some(sum) = &t.summary {
        s.push_str(&one_line(sum));
        s.push_str("\n\n");
    }
    if t.values.is_empty() {
        s.push_str("No controls.\n");
    } else {
        s.push_str(&table(&t.values));
    }
    if !t.refs.is_empty() {
        s.push_str(&format!("\nReferences: {}.\n", code_list(&t.refs)));
    }
    if !t.slots.is_empty() {
        s.push_str(&format!("\nSlots: {}.\n", code_list(&t.slots)));
    }
    if let Some(b) = t.bindable.as_ref().filter(|b| !b.is_empty()) {
        s.push_str("\nA binding from it carries:\n\n");
        s.push_str(&table(b));
    }
    s.push_str(END);
    s.push('\n');
    s
}

fn table(values: &[ValueSchema]) -> String {
    let mut s = String::from("| Control | Range | Default | What it does |\n|---|---|---|---|\n");
    for v in values {
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            cell(v.label.as_deref().unwrap_or(&v.key)),
            cell(&range(v)),
            cell(&default(v)),
            cell(v.description.as_deref().unwrap_or("")),
        ));
    }
    s
}

fn index(schema: &Schema, app: &str) -> Page {
    let heading = if app.is_empty() {
        "Nodes".to_string()
    } else {
        format!("{app} nodes")
    };
    let mut groups: BTreeMap<&str, Vec<&TypeSchema>> = BTreeMap::new();
    let mut other = Vec::new();
    for t in types(schema) {
        match &t.group {
            Some(g) => groups.entry(g.as_str()).or_default().push(t),
            None => other.push(t),
        }
    }
    let mut s = format!(
        "---\ntitle: {}\n---\n\n# {}\n\n<!-- generated whole by rhizome-docs; write in the node pages, not here -->\n",
        yaml(&heading),
        one_line(&heading)
    );
    let list = |s: &mut String, ts: &[&TypeSchema]| {
        s.push('\n');
        for t in ts {
            s.push_str(&format!("- [{}]({}.md)", one_line(title(t)), t.name));
            if let Some(sum) = &t.summary {
                s.push_str(&format!(": {}", one_line(sum)));
            }
            s.push('\n');
        }
    };
    for (g, ts) in &groups {
        s.push_str(&format!("\n## {}\n", capitalised(g)));
        list(&mut s, ts);
    }
    if !other.is_empty() {
        if !groups.is_empty() {
            s.push_str("\n## Other\n");
        }
        list(&mut s, &other);
    }
    Page {
        file_name: INDEX.to_string(),
        content: s,
    }
}

/// The span a control covers, with its unit.
fn range(v: &ValueSchema) -> String {
    let unit = v.unit.as_deref();
    match v.kind {
        ValueKind::Bool => "on or off".into(),
        ValueKind::Int | ValueKind::Float => match v.range {
            Some((min, max)) => {
                if let Some(names) = names_of(v) {
                    return names.join(", ");
                }
                let more = if v.soft_max { ", or more" } else { "" };
                format!("{} to {}{more}", scalar(min, unit), scalar(max, unit))
            }
            None => "any".into(),
        },
        ValueKind::Choice => v.choices.join(", "),
        ValueKind::Text => "text".into(),
        ValueKind::Vec2 => "x, y".into(),
        ValueKind::Vec3 => "x, y, z".into(),
        ValueKind::Colour => "colour".into(),
        ValueKind::Floats => match v.len {
            Some(n) => format!("{n} numbers"),
            None => "numbers".into(),
        },
        ValueKind::Shaped => "structured".into(),
    }
}

/// The default as a person reads it.
fn default(v: &ValueSchema) -> String {
    let unit = v.unit.as_deref();
    let d = &v.default;
    let numbers = |xs: &[Json]| {
        xs.iter()
            .map(|x| x.as_f64().map(number).unwrap_or_else(|| x.to_string()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (v.kind, d) {
        (ValueKind::Bool, Json::Bool(b)) => if *b { "on" } else { "off" }.into(),
        (ValueKind::Int | ValueKind::Float, Json::Number(n)) => {
            let x = n.as_f64().unwrap_or_default();
            match (names_of(v), n.as_i64()) {
                (Some(names), Some(i)) => {
                    let at = i - v.range.map_or(0.0, |r| r.0) as i64;
                    usize::try_from(at)
                        .ok()
                        .and_then(|at| names.get(at))
                        .cloned()
                        .unwrap_or_else(|| number(x))
                }
                _ => scalar(x, unit),
            }
        }
        (ValueKind::Text, Json::String(t)) if t.is_empty() => "empty".into(),
        (_, Json::String(t)) => t.clone(),
        (ValueKind::Colour, Json::Array(a)) => hex(a).unwrap_or_else(|| numbers(a)),
        (ValueKind::Floats, Json::Array(a)) if a.len() > 4 => format!("{} numbers", a.len()),
        (ValueKind::Vec2 | ValueKind::Vec3 | ValueKind::Floats, Json::Array(a)) => {
            with_unit(&numbers(a), unit)
        }
        (_, Json::Null) => "empty".into(),
        (_, Json::Array(a)) if a.is_empty() => "empty".into(),
        (_, Json::Object(o)) if o.is_empty() => "empty".into(),
        (_, other) => other.to_string(),
    }
}

fn hex(rgba: &[Json]) -> Option<String> {
    let c: Vec<u8> = rgba
        .iter()
        .map(|x| {
            x.as_f64()
                .map(|f| (f.clamp(0.0, 1.0) * 255.0).round() as u8)
        })
        .collect::<Option<_>>()?;
    let [r, g, b, a] = c[..] else {
        return None;
    };
    Some(if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    })
}

/// The names an int value's steps carry, when there is one for every step in its range.
fn names_of(v: &ValueSchema) -> Option<&[String]> {
    if v.kind != ValueKind::Int {
        return None;
    }
    v.names.as_deref().filter(|n| !n.is_empty())
}

/// A number without a trailing `.0` or float noise (`0.30000001` reads `0.3`).
fn number(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        return format!("{}", x as i64);
    }
    let s = format!("{x:.5}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "" | "-0" => "0".into(),
        s => s.to_string(),
    }
}

/// A single number as a person reads it, with its unit: a `%` value is stored as a fraction
/// and shown ×100; `Hz` from 1000 up reads as `kHz`, and `ms` from 1000 up as `s`.
fn scalar(x: f64, unit: Option<&str>) -> String {
    match unit {
        Some("%") => with_unit(&number(x * 100.0), unit),
        Some("Hz") if x.abs() >= 1000.0 => with_unit(&number(x / 1000.0), Some("kHz")),
        Some("ms") if x.abs() >= 1000.0 => with_unit(&number(x / 1000.0), Some("s")),
        _ => with_unit(&number(x), unit),
    }
}

/// `20 Hz`, but `50%` and `2×`: a unit that starts with a letter or digit gets a space.
fn with_unit(n: &str, unit: Option<&str>) -> String {
    match unit {
        Some(u) if u.starts_with(|c: char| c.is_alphanumeric()) => format!("{n} {u}"),
        Some(u) => format!("{n}{u}"),
        None => n.to_string(),
    }
}

fn code_list(items: &[String]) -> String {
    items
        .iter()
        .map(|i| format!("`{i}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A table cell: one line, its pipes escaped.
fn cell(s: &str) -> String {
    one_line(s).replace('|', "\\|")
}

/// A YAML scalar, quoted only when it has to be.
fn yaml(s: &str) -> String {
    let s = one_line(s);
    let plain = !s.is_empty()
        && !s.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c))
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.ends_with(':')
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
        )
        && s.parse::<f64>().is_err();
    if plain {
        s
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_units_read_naturally() {
        assert_eq!(number(20000.0), "20000");
        assert_eq!(number(0.25), "0.25");
        assert_eq!(with_unit("20", Some("Hz")), "20 Hz");
        assert_eq!(with_unit("50", Some("%")), "50%");
        assert_eq!(with_unit("2", Some("×")), "2×");
    }

    #[test]
    fn units_scale_and_noise_is_trimmed() {
        assert_eq!(scalar(0.3, Some("%")), "30%");
        assert_eq!(scalar(0.30000001, Some("%")), "30%");
        assert_eq!(scalar(20.0, Some("Hz")), "20 Hz");
        assert_eq!(scalar(20000.0, Some("Hz")), "20 kHz");
        assert_eq!(scalar(1500.0, Some("Hz")), "1.5 kHz");
        assert_eq!(scalar(10.0, Some("ms")), "10 ms");
        assert_eq!(scalar(2000.0, Some("ms")), "2 s");
        assert_eq!(number(0.30000001), "0.3");
        assert_eq!(number(-0.0000001), "0");
    }

    #[test]
    fn yaml_quotes_only_when_it_must() {
        assert_eq!(yaml("Echo"), "Echo");
        assert_eq!(yaml("A delay, then more."), "A delay, then more.");
        assert_eq!(yaml("Ratio: 2 to 1"), "\"Ratio: 2 to 1\"");
        assert_eq!(yaml("on"), "\"on\"");
        assert_eq!(yaml("say \"hi\""), "say \"hi\"");
        assert_eq!(yaml("\"quoted\""), "\"\\\"quoted\\\"\"");
    }

    #[test]
    fn cells_stay_on_one_line() {
        assert_eq!(cell("a | b\nc"), "a \\| b c");
    }
}
