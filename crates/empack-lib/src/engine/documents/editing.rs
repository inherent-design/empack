//! Bounded syntax edits. Unsupported changed syntax fails without a reformat fallback.
use super::*;
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_saphyr::Spanned;
use std::{fmt, ops::Range};

type Node = Spanned<Syntax>;
#[derive(Debug)]
enum Syntax {
    Scalar,
    Map(Vec<(Spanned<String>, Node)>),
    Seq(Vec<Node>),
}
impl<'de> Deserialize<'de> for Syntax {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Walk;
        impl<'de> Visitor<'de> for Walk {
            type Value = Syntax;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("YAML value")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Syntax, M::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Syntax::Map(entries))
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Syntax, S::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = seq.next_element()? {
                    entries.push(entry);
                }
                Ok(Syntax::Seq(entries))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Syntax, E> {
                Ok(Syntax::Scalar)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Syntax, E> {
                Ok(Syntax::Scalar)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Syntax, E> {
                Ok(Syntax::Scalar)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Syntax, E> {
                Ok(Syntax::Scalar)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Syntax, E> {
                Ok(Syntax::Scalar)
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Syntax, E> {
                Ok(Syntax::Scalar)
            }
        }
        d.deserialize_any(Walk)
    }
}
fn token<T>(node: &Spanned<T>) -> Result<Range<usize>> {
    ensure!(
        node.referenced == node.defined,
        "Cannot safely edit YAML aliases or merges"
    );
    let span = node.referenced.span();
    let start = span.byte_offset().context("Missing YAML byte offset")? as usize;
    let len = span.byte_len().context("Missing YAML byte length")? as usize;
    Ok(start..start + len)
}
fn extent(node: &Node) -> Result<Range<usize>> {
    let mut range = token(node)?;
    match &node.value {
        Syntax::Map(entries) => {
            for (key, value) in entries {
                range.end = range.end.max(token(key)?.end).max(extent(value)?.end);
            }
        }
        Syntax::Seq(entries) => {
            for value in entries {
                range.end = range.end.max(extent(value)?.end);
            }
        }
        Syntax::Scalar => (),
    }
    Ok(range)
}
struct Editor<'a> {
    text: &'a str,
    newline: &'static str,
    changes: Vec<(Range<usize>, String)>,
}
impl Editor<'_> {
    fn line_start(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map_or(0, |n| n + 1)
    }
    fn line_end(&self, pos: usize) -> usize {
        self.text[pos..]
            .find('\n')
            .map_or(self.text.len(), |n| pos + n + 1)
    }
    fn indent(&self, pos: usize) -> Result<usize> {
        let start = self.line_start(pos);
        ensure!(
            self.text[start..pos].bytes().all(|c| c == b' '),
            "Cannot safely edit non-block YAML mapping"
        );
        Ok(pos - start)
    }
    fn block(&self, value: &Value, indent: usize) -> Result<String> {
        let encoded = String::from_utf8(serialize(value)?)?;
        Ok(encoded
            .lines()
            .map(|line| format!("{}{line}{}", " ".repeat(indent), self.newline))
            .collect())
    }
    fn safe(&self, node: &Node) -> Result<()> {
        let range = token(node)?;
        ensure!(range.end <= self.text.len(), "Invalid YAML source span");
        let start = self.text[range.start..].chars().next();
        let empty = matches!(&node.value, Syntax::Map(v) if v.is_empty());
        ensure!(
            empty || !matches!(start, Some('[' | '{' | '*' | '&' | '!')),
            "Cannot safely edit flow, anchored or tagged YAML; expand the changed value first"
        );
        Ok(())
    }
    fn replace(&mut self, node: &Node, next: &Value, parent_indent: usize) -> Result<()> {
        self.safe(node)?;
        let mut range = extent(node)?;
        if matches!(&node.value, Syntax::Map(entries) if entries.is_empty()) {
            // Container spans identify only the opening token.
            let close = self.text[range.start..self.line_end(range.start)]
                .find('}')
                .context("Cannot safely locate empty YAML mapping")?;
            ensure!(
                self.text[range.start + 1..range.start + close]
                    .trim()
                    .is_empty(),
                "Cannot safely replace empty YAML mapping"
            );
            range.end = range.start + close + 1;
        }
        let scalar = matches!(node.value, Syntax::Scalar);
        let next_scalar = !next.is_object() && !next.is_array();
        if scalar && next_scalar {
            ensure!(
                !self.text[range.clone()].contains(['\n', '\r']),
                "Cannot safely edit multiline YAML scalar"
            );
            let encoded = String::from_utf8(serialize(next)?)?;
            ensure!(
                encoded.trim_end().lines().count() == 1,
                "Cannot safely insert multiline YAML scalar"
            );
            self.changes.push((range, encoded.trim_end().into()));
        } else if scalar || matches!(&node.value, Syntax::Map(v) if v.is_empty()) {
            let end = self.line_end(range.end);
            let suffix = self.text[range.end..end].trim();
            ensure!(
                suffix.is_empty() || suffix.starts_with('#'),
                "Cannot safely replace inline YAML value"
            );
            let replacement = format!(
                "{}{}{}",
                if suffix.is_empty() {
                    String::new()
                } else {
                    suffix.to_owned()
                },
                self.newline,
                self.block(next, parent_indent + 2)?
            );
            self.changes.push((range.start..end, replacement));
        } else {
            // A changed collection may be replaced only when it contains no comments.
            let start = self.line_start(range.start);
            let end = self.line_end(range.end);
            ensure!(
                !self.text[start..end].contains('#'),
                "Cannot safely replace commented YAML collection"
            );
            self.changes
                .push((start..end, self.block(next, parent_indent + 2)?));
        }
        Ok(())
    }
    fn diff(&mut self, node: &Node, old: &Value, next: &Value, parent_indent: usize) -> Result<()> {
        if old == next {
            return Ok(());
        }
        self.safe(node)?;
        if let (Syntax::Map(entries), Some(old), Some(next)) =
            (&node.value, old.as_object(), next.as_object())
        {
            if entries.is_empty() {
                return self.replace(node, &Value::Object(next.clone()), parent_indent);
            }
            let indent = self.indent(token(&entries[0].0)?.start)?;
            let mut additions = serde_json::Map::new();
            for key in old.keys().chain(next.keys()).collect::<BTreeSet<_>>() {
                if old.get(key) == next.get(key) {
                    continue;
                }
                let entry = entries.iter().find(|(name, _)| &name.value == key);
                match (entry, next.get(key)) {
                    (Some((name, value)), None) => {
                        let start = self.line_start(token(name)?.start);
                        let end = self.line_end(extent(value)?.end);
                        ensure!(
                            !self.text[start..end]
                                .lines()
                                .skip(1)
                                .any(|l| l.trim_start().starts_with('#')),
                            "Cannot safely remove commented YAML collection"
                        );
                        self.changes.push((start..end, String::new()));
                    }
                    (Some((name, value)), Some(next)) => {
                        let gap = &self.text[token(name)?.end..token(value)?.start];
                        ensure!(
                            !gap.contains(['&', '*', '!']),
                            "Cannot safely edit anchored or tagged YAML"
                        );
                        self.diff(value, old.get(key).unwrap_or(&Value::Null), next, indent)?;
                    }
                    (None, Some(next)) => {
                        additions.insert(key.clone(), next.clone());
                    }
                    (None, None) => (),
                }
            }
            if !additions.is_empty() || next.is_empty() {
                let end = self.line_end(extent(node)?.end);
                let prefix = if end > 0 && !self.text[..end].ends_with('\n') {
                    self.newline
                } else {
                    ""
                };
                self.changes.push((
                    end..end,
                    format!("{prefix}{}", self.block(&Value::Object(additions), indent)?),
                ));
            }
            Ok(())
        } else {
            self.replace(node, next, parent_indent)
        }
    }
}

pub(super) fn patch(original: &[u8], old: &[u8], next: &[u8]) -> Result<Vec<u8>> {
    let old = parse(old)?;
    let next = parse(next)?;
    if old == next {
        return Ok(original.to_vec());
    }
    let text = std::str::from_utf8(original)?;
    let syntax: Node = serde_saphyr::from_str(text)?;
    let mut editor = Editor {
        text,
        newline: if text.contains("\r\n") { "\r\n" } else { "\n" },
        changes: Vec::new(),
    };
    editor.diff(&syntax, &old, &next, 0)?;
    editor
        .changes
        .sort_by_key(|(range, _)| (range.start, range.end));
    ensure!(
        editor
            .changes
            .windows(2)
            .all(|pair| pair[0].0.end <= pair[1].0.start),
        "Overlapping YAML edits"
    );
    let mut output = original.to_vec();
    for (range, replacement) in editor.changes.into_iter().rev() {
        output.splice(range, replacement.bytes());
    }
    ensure!(
        output.len() <= MAX_DOCUMENT_BYTES,
        "Document exceeds size limit"
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    const INPUT: &str = "# café\nschema: 5\npack:\n  id: demo\n  name: 'Démo' # retain name\n  version: one # release\nruntime:\n  minecraft: '1.21.1'\n  loader: fabric\n  java: 21\ndependencies:\n  # Keep this provider spelling\n  alpha: modrinth:Alpha001 # alpha note\n  beta: modrinth:Beta0001 # beta note\n";
    #[test]
    fn metadata_preserves_comments_unicode_crlf_and_defaults() {
        for input in [INPUT.to_owned(), INPUT.replace('\n', "\r\n")] {
            let source = DocumentCodec
                .decode_intent(input.as_bytes(), "test")
                .unwrap();
            let mut next = source.intent().clone();
            next.metadata.version = "two".into();
            let edit = DocumentCodec.replace_intent(&source, &next).unwrap();
            assert_eq!(
                String::from_utf8(edit.bytes).unwrap(),
                input.replace("version: one", "version: two")
            );
            assert_eq!(edit.expected, source.raw_revision());
        }
    }
    #[test]
    fn shorthand_pin_keeps_neighbor_comments() {
        let source = DocumentCodec
            .decode_intent(INPUT.as_bytes(), "test")
            .unwrap();
        let changed = INPUT.replace(
            "alpha: modrinth:Alpha001 # alpha note",
            "alpha:\n    source: modrinth:Alpha001\n    pin: Version1",
        );
        let next = DocumentCodec
            .decode_intent(changed.as_bytes(), "next")
            .unwrap();
        let edit = DocumentCodec
            .replace_intent(&source, next.intent())
            .unwrap();
        let text = String::from_utf8(edit.bytes.clone()).unwrap();
        assert!(text.contains("alpha: # alpha note\n"), "{text}");
        assert!(text.contains("  beta: modrinth:Beta0001 # beta note\n"));
        assert!(text.starts_with("# café\n"));
        assert_eq!(edit.decode("check").unwrap().intent(), next.intent());
    }
    #[test]
    fn add_and_remove_entries_keep_neighbor_comments() {
        let source = DocumentCodec
            .decode_intent(INPUT.as_bytes(), "test")
            .unwrap();
        let changed = INPUT
            .replace("  alpha: modrinth:Alpha001 # alpha note\n", "")
            .replace("  beta:", "  gamma: modrinth:Gamma001\n  beta:");
        let next = DocumentCodec
            .decode_intent(changed.as_bytes(), "next")
            .unwrap();
        let edit = DocumentCodec
            .replace_intent(&source, next.intent())
            .unwrap();
        let text = String::from_utf8(edit.bytes.clone()).unwrap();
        assert!(text.contains("  # Keep this provider spelling\n"));
        assert!(text.contains("  beta: modrinth:Beta0001 # beta note\n"));
        assert_eq!(edit.decode("check").unwrap().intent(), next.intent());
    }
    #[test]
    fn edits_both_documents_and_retains_binding_revision() {
        let bindings =
            b"# integrations\nschema: 1\nbindings: {}\npolicies:\n  config/a.toml: seed # policy\n";
        let source = DocumentCodec
            .decode_authoring(INPUT.as_bytes(), Some(bindings), "test")
            .unwrap();
        let mut next = source.intent().clone();
        next.metadata.version = "two".into();
        next.installation_policies.clear();
        let edit = DocumentCodec.replace_intent(&source, &next).unwrap();
        assert_eq!(edit.bindings_expected, source.bindings_revision());
        assert!(
            edit.bindings
                .as_ref()
                .unwrap()
                .starts_with(b"# integrations\n")
        );
        assert_eq!(edit.decode("check").unwrap().intent(), &next);
    }
    #[test]
    fn empty_maps_support_addition_and_last_removal() {
        let source = DocumentCodec
            .decode_intent(INPUT.as_bytes(), "test")
            .unwrap();
        let mut next = source.intent().clone();
        next.roots.clear();
        let removed = DocumentCodec.replace_intent(&source, &next).unwrap();
        let empty = removed.decode("empty").unwrap();
        assert!(
            String::from_utf8(removed.bytes)
                .unwrap()
                .contains("# Keep this provider spelling")
        );
        let restored = DocumentCodec
            .replace_intent(&empty, source.intent())
            .unwrap();
        assert_eq!(
            restored.decode("restored").unwrap().intent(),
            source.intent()
        );
    }
    #[test]
    fn normalized_defaults_do_not_rewrite_explicit_forms() {
        let input = INPUT
            .replace("  loader: fabric", "  loader:\n    kind: fabric")
            .replace(
                "  version: one",
                "  authors: [] # explicit default\n  version: one",
            );
        let source = DocumentCodec
            .decode_intent(input.as_bytes(), "test")
            .unwrap();
        let mut next = source.intent().clone();
        next.metadata.version = "two".into();
        let edit = DocumentCodec.replace_intent(&source, &next).unwrap();
        assert_eq!(
            String::from_utf8(edit.bytes).unwrap(),
            input.replace("version: one", "version: two")
        );
    }
    #[test]
    fn refuses_changed_flow_and_alias_but_preserves_noop() {
        for input in [
            INPUT.replace("  version: one # release", "  version: &v one # release"),
            INPUT.replace(
                "  id: demo\n  name: 'Démo' # retain name\n  version: one # release",
                "  {id: demo, name: Démo, version: one}",
            ),
        ] {
            let source = DocumentCodec
                .decode_intent(input.as_bytes(), "test")
                .unwrap();
            assert_eq!(
                DocumentCodec
                    .replace_intent(&source, source.intent())
                    .unwrap()
                    .bytes,
                input.as_bytes()
            );
            let mut next = source.intent().clone();
            next.metadata.version = "two".into();
            assert!(
                DocumentCodec.replace_intent(&source, &next).is_err(),
                "{input}"
            );
        }
    }
}
