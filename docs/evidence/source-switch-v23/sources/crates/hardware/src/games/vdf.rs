//! Bounded KeyValues text reader; keeps hierarchy and decodes quoted escapes.
#[derive(Debug)]
pub(super) enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}

pub(super) fn text<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    entries.iter().find_map(|(name, value)| match value {
        Value::Text(value) if name.eq_ignore_ascii_case(key) => Some(value.as_str()),
        _ => None,
    })
}

pub(super) fn object<'a>(
    entries: &'a [(String, Value)],
    key: &str,
) -> Option<&'a [(String, Value)]> {
    entries.iter().find_map(|(name, value)| match value {
        Value::Object(value) if name.eq_ignore_ascii_case(key) => Some(value.as_slice()),
        _ => None,
    })
}

pub(super) fn parse(input: &str) -> Option<Vec<(String, Value)>> {
    let mut reader = Reader {
        chars: input.trim_start_matches('\u{feff}').chars().peekable(),
        nodes: 0,
    };
    reader.entries(0, false)
}

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    nodes: usize,
}
impl Reader<'_> {
    fn whitespace(&mut self) {
        loop {
            while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
                self.chars.next();
            }
            let mut look = self.chars.clone();
            if look.next() == Some('/') && look.next() == Some('/') {
                for c in self.chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }
    fn token(&mut self) -> Option<String> {
        self.whitespace();
        if self.chars.peek() == Some(&'"') {
            self.chars.next();
            let mut value = String::new();
            while let Some(c) = self.chars.next() {
                match c {
                    '"' => return Some(value),
                    '\\' => match self.chars.next()? {
                        'n' => value.push('\n'),
                        't' => value.push('\t'),
                        'r' => value.push('\r'),
                        c @ ('\\' | '"') => value.push(c),
                        c => {
                            value.push('\\');
                            value.push(c);
                        }
                    },
                    c => value.push(c),
                }
            }
            None
        } else {
            let mut value = String::new();
            while let Some(&c) = self.chars.peek() {
                if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                    break;
                }
                value.push(c);
                self.chars.next();
            }
            (!value.is_empty()).then_some(value)
        }
    }
    fn entries(&mut self, depth: usize, nested: bool) -> Option<Vec<(String, Value)>> {
        if depth > 32 {
            return None;
        }
        let mut entries = Vec::new();
        loop {
            self.whitespace();
            match self.chars.peek() {
                None => return (!nested).then_some(entries),
                Some('}') if nested => {
                    self.chars.next();
                    return Some(entries);
                }
                _ => {}
            }
            self.nodes += 1;
            if self.nodes > 100_000 {
                return None;
            }
            let key = self.token()?;
            self.whitespace();
            let value = if self.chars.peek() == Some(&'{') {
                self.chars.next();
                Value::Object(self.entries(depth + 1, true)?)
            } else {
                Value::Text(self.token()?)
            };
            entries.push((key, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_compact_nested_unicode_escapes_and_comments() {
        let entries = parse("// fixture\n\"AppState\" { \"appid\" \"620\" \"name\" \"Café \\\"二\\\"\\\\path\" \"nested\" {\"name\" \"wrong\"} }").unwrap();
        let state = object(&entries, "appstate").unwrap();
        assert_eq!(text(state, "name"), Some("Café \"二\"\\path"));
        assert_eq!(text(state, "appid"), Some("620"));
    }
    #[test]
    fn rejects_truncation_and_excessive_nesting() {
        for input in ["\"a\" {", "\"a\" \"unfinished", "}", "\"a\""] {
            assert!(parse(input).is_none());
        }
        assert!(parse(&format!("{}{}", "a {".repeat(40), "}".repeat(40))).is_none());
    }
}
