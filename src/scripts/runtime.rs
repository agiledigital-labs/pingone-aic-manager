//! Calls to members the AM script engine does not have.
//!
//! A syntax check cannot catch these: `Object.values(o)` parses everywhere,
//! and on AM throws `TypeError: Cannot find function values in object
//! function Object()` when the line runs — in a journey, far from the push.
//! The members below were each probed on the AM engine and found absent, with
//! a present control beside them (`docs/api/12-script-bindings-matrix.md`,
//! "ES2017 `Object` members" and "ES2015 global objects"). They are present on
//! the IDM engine, which is why the check is AM-only.
//!
//! The scan is lexical, not a parse: comments, strings and template literals
//! are blanked first, and only **calls** are flagged — `typeof Object.values`
//! and `if (Object.values) …` are feature detection, which is the correct way
//! to use them, and stay legal. A name the source declares itself (`var Map =
//! …`) is not the global and is not flagged.

/// One absent member, where the source calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Miss {
    pub member: &'static str,
    /// 1-based, indexing the decoded source.
    pub line: u32,
    pub column: u32,
    pub instead: &'static str,
}

impl Miss {
    pub fn render(&self) -> String {
        format!(
            "line {}:{}: {} is not on the AM engine — {}",
            self.line, self.column, self.member, self.instead
        )
    }
}

/// How an absent member is reached in a call.
enum Shape {
    /// `Object.values(` — a member of a present global.
    Member(&'static str, &'static str),
    /// `new Map(` — an absent global constructed.
    Construct(&'static str),
    /// `Symbol(` / `Promise.resolve(` — an absent global called or used as a
    /// namespace.
    Global(&'static str),
}

/// Each absent member: how it is reached, how it is named, and what to use.
const ABSENT: &[(Shape, &str, &str)] = &[
    (
        Shape::Member("Object", "values"),
        "Object.values",
        "use Object.keys(o).map(function (k) { return o[k]; })",
    ),
    (
        Shape::Member("Object", "entries"),
        "Object.entries",
        "use Object.keys(o).map(function (k) { return [k, o[k]]; })",
    ),
    (
        Shape::Member("Object", "getOwnPropertyDescriptors"),
        "Object.getOwnPropertyDescriptors",
        "use Object.getOwnPropertyDescriptor per key",
    ),
    (
        Shape::Construct("Map"),
        "new Map",
        "use a plain object for string keys",
    ),
    (
        Shape::Construct("Set"),
        "new Set",
        "use a plain object as a set (seen[k] = true)",
    ),
    (
        Shape::Construct("WeakMap"),
        "new WeakMap",
        "use a plain object",
    ),
    (
        Shape::Construct("WeakSet"),
        "new WeakSet",
        "use a plain object",
    ),
    (
        Shape::Construct("Promise"),
        "new Promise",
        "AM scripts are synchronous; call the function directly",
    ),
    (
        Shape::Global("Promise"),
        "Promise",
        "AM scripts are synchronous; call the function directly",
    ),
    (Shape::Global("Symbol"), "Symbol", "use a string key"),
    (
        Shape::Global("Reflect"),
        "Reflect",
        "use Object.* or direct property access",
    ),
    (
        Shape::Construct("Proxy"),
        "new Proxy",
        "wrap the object in a function instead",
    ),
];

/// Every call to an absent member in `source`, in source order.
pub fn am_misses(source: &str) -> Vec<Miss> {
    let code = blank_non_code(source);
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    for (shape, member, instead) in ABSENT {
        let needle = match shape {
            Shape::Member(object, _) => *object,
            Shape::Construct(global) | Shape::Global(global) => *global,
        };
        if declares(&code, needle) {
            continue;
        }
        for (at, _) in code.match_indices(needle) {
            if !word_starts_at(bytes, at) || !word_ends_at(bytes, at + needle.len()) {
                continue;
            }
            let after = skip_ws(bytes, at + needle.len());
            let hit = match shape {
                Shape::Member(_, member) => {
                    bytes.get(after) == Some(&b'.')
                        && word_at(bytes, skip_ws(bytes, after + 1)) == Some(*member)
                        && bytes.get(skip_ws(bytes, skip_ws(bytes, after + 1) + member.len()))
                            == Some(&b'(')
                }
                Shape::Construct(_) => preceded_by_new(bytes, at),
                Shape::Global(_) => {
                    !preceded_by_new(bytes, at)
                        && (bytes.get(after) == Some(&b'(') || bytes.get(after) == Some(&b'.'))
                        && !preceded_by_typeof(bytes, at)
                }
            };
            if hit {
                let (line, column) = position(source, at);
                out.push(Miss {
                    member,
                    line,
                    column,
                    instead,
                });
            }
        }
    }
    out.sort_by_key(|miss| (miss.line, miss.column));
    out
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// Not preceded by an identifier character or a `.` — so `java.util.Map`,
/// `myObject.values` and `HashMap` are not the global.
fn word_starts_at(bytes: &[u8], at: usize) -> bool {
    let mut i = at;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    let before_ws = (i > 0).then(|| bytes[i - 1]);
    let immediate = (at > 0).then(|| bytes[at - 1]);
    !immediate.is_some_and(is_ident) && before_ws != Some(b'.')
}

fn word_ends_at(bytes: &[u8], end: usize) -> bool {
    !bytes.get(end).copied().is_some_and(is_ident)
}

fn word_at(bytes: &[u8], at: usize) -> Option<&str> {
    let end = (at..bytes.len())
        .find(|&i| !is_ident(bytes[i]))
        .unwrap_or(bytes.len());
    (end > at).then(|| std::str::from_utf8(&bytes[at..end]).ok())?
}

fn skip_ws(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at
}

fn previous_word(bytes: &[u8], at: usize) -> Option<&str> {
    let mut end = at;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    (start < end).then(|| std::str::from_utf8(&bytes[start..end]).ok())?
}

fn preceded_by_new(bytes: &[u8], at: usize) -> bool {
    previous_word(bytes, at) == Some("new")
}

fn preceded_by_typeof(bytes: &[u8], at: usize) -> bool {
    previous_word(bytes, at) == Some("typeof")
}

/// Whether the source binds `name` itself — `var`/`let`/`const`/`function`
/// `name`, or a parameter list is beyond a lexical scan, so only declarations.
fn declares(code: &str, name: &str) -> bool {
    let bytes = code.as_bytes();
    code.match_indices(name).any(|(at, _)| {
        word_starts_at(bytes, at)
            && word_ends_at(bytes, at + name.len())
            && matches!(
                previous_word(bytes, at),
                Some("var" | "let" | "const" | "function")
            )
    })
}

/// 1-based line and column of byte `at`, counting columns in characters.
fn position(source: &str, at: usize) -> (u32, u32) {
    let before = &source[..at];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    (line as u32, column as u32)
}

/// `source` with comments and the contents of strings and template literals
/// replaced by spaces, byte for byte, so offsets still index the original.
/// Newlines are kept so line numbers survive.
fn blank_non_code(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Code,
        Line,
        Block,
        Quote(u8),
    }
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut state = State::Code;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        let next = bytes.get(i + 1).copied();
        let blank = |b: u8| if b == b'\n' { b'\n' } else { b' ' };
        match state {
            State::Code => match (byte, next) {
                (b'/', Some(b'/')) => {
                    state = State::Line;
                    out.extend(*b"  ");
                    i += 2;
                    continue;
                }
                (b'/', Some(b'*')) => {
                    state = State::Block;
                    out.extend(*b"  ");
                    i += 2;
                    continue;
                }
                (b'"' | b'\'' | b'`', _) => {
                    state = State::Quote(byte);
                    out.push(b' ');
                }
                _ => out.push(byte),
            },
            State::Line => {
                if byte == b'\n' {
                    state = State::Code;
                }
                out.push(blank(byte));
            }
            State::Block => {
                if byte == b'*' && next == Some(b'/') {
                    state = State::Code;
                    out.extend(*b"  ");
                    i += 2;
                    continue;
                }
                out.push(blank(byte));
            }
            State::Quote(quote) => {
                if byte == b'\\' {
                    out.push(b' ');
                    if let Some(escaped) = next {
                        out.push(blank(escaped));
                    }
                    i += 2;
                    continue;
                }
                // A newline ends a quote or double-quote string, which
                // cannot span lines unescaped: this bounds the damage when a
                // regex literal such as /'/ is misread as opening a string.
                if byte == quote || (byte == b'\n' && quote != b'`') {
                    state = State::Code;
                }
                out.push(blank(byte));
            }
        }
        i += 1;
    }
    // Only ASCII bytes were replaced, by ASCII, and multi-byte sequences
    // inside code were copied whole; inside comments and strings each of
    // their bytes became a space, which is valid on its own.
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members(source: &str) -> Vec<&'static str> {
        am_misses(source).iter().map(|miss| miss.member).collect()
    }

    /// The case from the report: a decision node and its library calling
    /// both, which parse cleanly and throw when run.
    #[test]
    fn calls_are_found_with_their_position() {
        let source =
            "var o = {a: 1};\nvar v = Object.values(o);\n  Object.entries (o).forEach(f);\n";
        let found = am_misses(source);
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].member, found[0].line, found[0].column),
            ("Object.values", 2, 9)
        );
        assert_eq!(
            (found[1].member, found[1].line, found[1].column),
            ("Object.entries", 3, 3)
        );
        assert!(found[0].render().contains("Object.keys"));
    }

    /// Discriminating: the same names that are *not* calls to the absent
    /// global. A matcher on the bare text flags every one of these.
    #[test]
    fn feature_detection_comments_strings_and_other_objects_are_not_calls() {
        let source = r#"
            if (typeof Object.values === "function") { x = 1; }
            if (Object.entries) { y = 1; }
            // Object.values(o) in a comment
            /* new Map() in a block */
            var s = "Object.entries(o)";
            var t = `new Set()`;
            var u = 'Symbol(1)';
            var m = new java.util.HashMap();
            var n = new JavaImporter(java.util.Map);
            var w = record.values(o);
            var z = myObject.entries(o);
            var keys = Object.keys(o);
            var p = typeof Promise;
        "#;
        assert_eq!(members(source), Vec::<&str>::new());
    }

    #[test]
    fn absent_globals_are_found_when_constructed_or_called() {
        let source = "var a = new Map();\nvar b = new Set([1]);\nvar c = Symbol('k');\nPromise.resolve(1);\nvar d = Reflect.ownKeys(o);\n";
        assert_eq!(
            members(source),
            vec!["new Map", "new Set", "Symbol", "Promise", "Reflect"]
        );
    }

    #[test]
    fn a_name_the_source_declares_is_not_the_global() {
        assert!(members("function Map() {}\nvar m = new Map();").is_empty());
        assert!(members("var Set = Java.type('java.util.HashSet');\nnew Set();").is_empty());
        // Declaring one name does not excuse another.
        assert_eq!(members("var Map = {};\nnew Set();"), vec!["new Set"]);
    }

    #[test]
    fn escapes_and_unicode_keep_positions() {
        let source = "var s = \"a\\\"Object.values(o)\";\nvar é = 1; Object.values(o);";
        let found = am_misses(source);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].line, found[0].column), (2, 12));
    }
}
