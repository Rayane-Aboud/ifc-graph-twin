//! Small, dependency-free helpers for pulling a handful of attributes out of a
//! raw STEP entity record (ISO 10303-21, the encoding IFC uses).
//!
//! Deliberately **not** a full STEP parser — `ifc-lite-core` does the heavy,
//! performance-critical entity scanning. These helpers only run on the short
//! attribute slice of the few rooted entities we care about (spatial structure
//! and relationships), extracting GlobalIds and entity references. Because they
//! are pure and take `&str`, they are unit-tested here with no ifc-lite
//! dependency and no toolchain assumptions.
//!
//! They are careful about the two things that make naive comma-splitting wrong
//! in STEP: single-quoted strings (which may contain commas and parentheses, and
//! escape a quote by doubling it, `''`), and nested parenthesised lists.

/// Return the slice **inside** the outermost parentheses of an entity record.
/// `#12=IFCWALL('g',$,(#1,#2));` → `'g',$,(#1,#2)`. String- and nesting-aware.
pub fn attribute_list(record: &str) -> Option<&str> {
    let bytes = record.as_bytes();
    let open = bytes.iter().position(|&b| b == b'(')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut i = open;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\'' {
                // A doubled '' inside a string is an escaped quote, not a close.
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_str = false;
            }
        } else {
            match b {
                b'\'' => in_str = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&record[open + 1..i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Split an attribute list into its top-level fields, honoring strings and
/// nested lists. `'g',$,(#1,#2)` → `["'g'", "$", "(#1,#2)"]`.
pub fn split_top_level(list: &str) -> Vec<String> {
    let bytes = list.as_bytes();
    let mut fields = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_str = false;
            }
        } else {
            match b {
                b'\'' => in_str = true,
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    fields.push(list[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    fields.push(list[start..].trim().to_string());
    fields
}

/// If a field is a STEP string literal, return its unescaped contents.
/// `'2O2Fr...''X'` → `2O2Fr...'X`. Non-string fields (`$`, `#5`, `.T.`) → `None`.
pub fn as_step_string(field: &str) -> Option<String> {
    let f = field.trim();
    let inner = f.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner.replace("''", "'"))
}

/// Parse a single entity reference field, `#123` → `123`.
pub fn parse_ref(field: &str) -> Option<u32> {
    field.trim().strip_prefix('#')?.parse().ok()
}

/// Parse a list field of references, `(#1,#2,#3)` → `[1,2,3]`. Tolerates a bare
/// single ref and ignores `$`/non-ref entries.
pub fn parse_ref_list(field: &str) -> Vec<u32> {
    let f = field.trim();
    let inner = f
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(f);
    inner.split(',').filter_map(parse_ref).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_attribute_list_with_nested_and_strings() {
        let rec = "#12=IFCWALL('gu,id',$,(#1,#2),'a''b');";
        let list = attribute_list(rec).unwrap();
        let fields = split_top_level(list);
        assert_eq!(fields, vec!["'gu,id'", "$", "(#1,#2)", "'a''b'"]);
    }

    #[test]
    fn unescapes_step_string() {
        assert_eq!(as_step_string("'a''b'").as_deref(), Some("a'b"));
        assert_eq!(as_step_string("$"), None);
        assert_eq!(as_step_string("#5"), None);
    }

    #[test]
    fn parses_refs() {
        assert_eq!(parse_ref("#123"), Some(123));
        assert_eq!(parse_ref("$"), None);
        assert_eq!(parse_ref_list("(#1,#2,#3)"), vec![1, 2, 3]);
        assert_eq!(parse_ref_list("(#7)"), vec![7]);
        assert_eq!(parse_ref_list("$"), Vec::<u32>::new());
    }

    #[test]
    fn rel_aggregates_attribute_positions() {
        // IfcRelAggregates(GlobalId, OwnerHistory, Name, Description,
        //                  RelatingObject, RelatedObjects)
        let rec = "#40=IFCRELAGGREGATES('guid',#5,$,$,#10,(#11,#12));";
        let fields = split_top_level(attribute_list(rec).unwrap());
        assert_eq!(parse_ref(&fields[4]), Some(10)); // relating (parent)
        assert_eq!(parse_ref_list(&fields[5]), vec![11, 12]); // related (children)
    }

    #[test]
    fn rel_contained_attribute_positions() {
        // IfcRelContainedInSpatialStructure(GlobalId, OwnerHistory, Name,
        //     Description, RelatedElements, RelatingStructure)
        let rec = "#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('guid',#5,$,$,(#21,#22),#20);";
        let fields = split_top_level(attribute_list(rec).unwrap());
        assert_eq!(parse_ref_list(&fields[4]), vec![21, 22]); // related elements
        assert_eq!(parse_ref(&fields[5]), Some(20)); // relating structure
    }
}
