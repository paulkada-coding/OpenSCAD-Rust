//! Minimal JSON value, parser and pretty printer for the geometry expected
//! files (no external dependency needed for such small documents).

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Keys keep their insertion order (stable, diff-friendly output).
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Pretty prints with 2-space indentation; arrays of scalars stay on
    /// one line. Ends with a newline.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        write_value(self, 0, &mut out);
        out.push('\n');
        out
    }
}

fn is_scalar(v: &Json) -> bool {
    !matches!(v, Json::Arr(_) | Json::Obj(_))
}

fn write_value(v: &Json, indent: usize, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => {
            if n.is_finite() {
                out.push_str(&mesh_io::format_shortest(*n));
            } else {
                out.push_str("null");
            }
        }
        Json::Str(s) => write_string(s, out),
        Json::Arr(items) => {
            if items.iter().all(is_scalar) {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_value(item, indent, out);
                }
                out.push(']');
            } else {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(indent + 1, out);
                    write_value(item, indent + 1, out);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                push_indent(indent, out);
                out.push(']');
            }
        }
        Json::Obj(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (k, val)) in fields.iter().enumerate() {
                push_indent(indent + 1, out);
                write_string(k, out);
                out.push_str(": ");
                write_value(val, indent + 1, out);
                out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
            }
            push_indent(indent, out);
            out.push('}');
        }
    }
}

fn push_indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses a JSON document.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, msg: &str) -> String {
        format!("JSON: {msg} at byte {}", self.i)
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", c as char)))
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end")),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut fields = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.eat(b':')?;
                    fields.push((key, self.value()?));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            Some(_) => self.number(),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while self.i < self.s.len()
            && matches!(
                self.s[self.i],
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
            )
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse::<f64>().ok())
            .map(Json::Num)
            .ok_or_else(|| self.err("invalid number"))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.s.get(self.i) != Some(&b'"') {
            return Err(self.err("expected string"));
        }
        self.i += 1;
        let mut bytes = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err(self.err("unterminated string"));
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return Err(self.err("unterminated escape"));
                    };
                    self.i += 1;
                    match e {
                        b'"' => bytes.push(b'"'),
                        b'\\' => bytes.push(b'\\'),
                        b'/' => bytes.push(b'/'),
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'b' => bytes.push(8),
                        b'f' => bytes.push(12),
                        b'u' => {
                            let hex = self
                                .s
                                .get(self.i..self.i + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .ok_or_else(|| self.err("invalid \\u escape"))?;
                            self.i += 4;
                            let ch = char::from_u32(hex).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                }
                c => bytes.push(c),
            }
        }
        String::from_utf8(bytes).map_err(|_| self.err("invalid UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let v = Json::Obj(vec![
            ("a".into(), Json::Num(1.5)),
            ("b".into(), Json::Str("x\n\"y\"\u{1}".into())),
            (
                "c".into(),
                Json::Arr(vec![Json::Num(1e-7), Json::Null, Json::Bool(true)]),
            ),
            (
                "d".into(),
                Json::Arr(vec![Json::Arr(vec![Json::Num(0.0), Json::Num(-2.0)])]),
            ),
            ("e".into(), Json::Obj(vec![])),
        ]);
        let text = v.to_pretty();
        assert_eq!(
            text,
            "{\n  \"a\": 1.5,\n  \"b\": \"x\\n\\\"y\\\"\\u0001\",\n  \"c\": [1e-7, null, true],\n  \"d\": [\n    [0, -2]\n  ],\n  \"e\": {}\n}\n"
        );
        assert_eq!(parse(&text).unwrap(), v);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("{\"a\": }").is_err());
        assert!(parse("[1, 2").is_err());
        assert!(parse("1 2").is_err());
    }
}
