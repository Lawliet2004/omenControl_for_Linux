//! Bounded Valve binary KeyValues reader for Steam's shortcut inventory.
//! Executable/launch-option strings are data, never reconstructed commands.
#[derive(Debug)]
pub(super) enum Value {
    Object(Vec<(String, Value)>),
    Text(String),
    Int(u32),
    Other,
}

pub(super) fn unique<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    let mut matching = entries
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(key));
    let value = &matching.next()?.1;
    matching.next().is_none().then_some(value)
}

pub(super) fn parse(input: &[u8]) -> Option<Vec<(String, Value)>> {
    if input.len() > 8 * 1024 * 1024 {
        return None;
    }
    let mut reader = Reader {
        input,
        offset: 0,
        nodes: 0,
    };
    let entries = reader.entries(0)?;
    (reader.offset == input.len()).then_some(entries)
}

struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
    nodes: usize,
}
impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> Option<&[u8]> {
        let end = self.offset.checked_add(count)?;
        let bytes = self.input.get(self.offset..end)?;
        self.offset = end;
        Some(bytes)
    }
    fn string(&mut self) -> Option<String> {
        let length = self
            .input
            .get(self.offset..)?
            .iter()
            .position(|b| *b == 0)?;
        let value = std::str::from_utf8(self.bytes(length)?).ok()?.to_owned();
        self.bytes(1)?;
        Some(value)
    }
    fn entries(&mut self, depth: usize) -> Option<Vec<(String, Value)>> {
        if depth > 32 {
            return None;
        }
        let mut entries = Vec::new();
        loop {
            let kind = self.bytes(1)?[0];
            if kind == 8 {
                return Some(entries);
            }
            self.nodes += 1;
            if self.nodes > 100_000 {
                return None;
            }
            let name = self.string()?;
            let value = match kind {
                0 => Value::Object(self.entries(depth + 1)?),
                1 => Value::Text(self.string()?),
                2 => Value::Int(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?)),
                3 | 6 => {
                    self.bytes(4)?;
                    Value::Other
                }
                7 => {
                    self.bytes(8)?;
                    Value::Other
                }
                // Unknown/unsupported scalar types cannot be skipped safely.
                _ => return None,
            };
            entries.push((name, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_unsigned_ids_unicode_and_unknown_scalars() {
        let mut data = b"\x00shortcuts\0\x000\0\x02appid\0".to_vec();
        data.extend(0xf1234567u32.to_le_bytes());
        data.extend(
            b"\x01AppName\0Caf\xc3\xa9 \xe4\xba\x8c\0\x00tags\0\x010\0tag\0\x08\x08\x08\x08",
        );
        let root = parse(&data).unwrap();
        let Some(Value::Object(shortcuts)) = unique(&root, "shortcuts") else {
            panic!()
        };
        let Some(Value::Object(game)) = unique(shortcuts, "0") else {
            panic!()
        };
        assert!(matches!(
            unique(game, "appid"),
            Some(Value::Int(0xf1234567))
        ));
        assert!(matches!(unique(game,"AppName"),Some(Value::Text(name)) if name=="Café 二"));
        for end in 0..data.len() {
            assert!(parse(&data[..end]).is_none());
        }
        data.push(8);
        assert!(parse(&data).is_none());
        assert!(parse(b"\x05name\0\x08").is_none());
        assert!(unique(
            &[("key".into(), Value::Int(1)), ("KEY".into(), Value::Int(2))],
            "key"
        )
        .is_none());
    }
    #[test]
    fn bounded_depth_nodes_and_file_size() {
        let mut deep = b"\x00x\0".repeat(34);
        deep.extend(vec![8; 35]);
        assert!(parse(&deep).is_none());
        let mut many = b"\x01x\0y\0".repeat(100_001);
        many.push(8);
        assert!(parse(&many).is_none());
        assert!(parse(&vec![0; 8 * 1024 * 1024 + 1]).is_none());
    }
}
