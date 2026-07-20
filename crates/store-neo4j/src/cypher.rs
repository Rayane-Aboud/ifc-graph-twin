//! Cypher statement builders.
//!
//! Values are passed as **parameters**, never string-interpolated, so the
//! generated statements are injection-safe and driver-ready. The two things
//! Cypher cannot parameterize — node *labels* and relationship *types* — are
//! derived from the [`Term`] and sanitized to a safe identifier before being
//! inlined.

use twin_core::{Edge, Node, PropertyValue, Term};

/// A ready-to-execute Cypher statement with its bound parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct CypherStatement {
    pub query: String,
    pub params: Vec<(String, ParamValue)>,
}

/// Driver-neutral parameter value, mirroring [`PropertyValue`] plus the scalars
/// the builders need for keys.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Int(i64),
    Null,
}

impl From<&PropertyValue> for ParamValue {
    fn from(v: &PropertyValue) -> Self {
        match v {
            PropertyValue::Text(s) => ParamValue::Text(s.clone()),
            PropertyValue::Number(n) => ParamValue::Number(*n),
            PropertyValue::Bool(b) => ParamValue::Bool(*b),
            // Neo4j has no unit type, so the magnitude alone maps here; the unit
            // is emitted as a companion `<key>__unit` property in `merge_node`.
            PropertyValue::Quantity { value, .. } => ParamValue::Number(*value),
        }
    }
}

/// `MERGE` a node on its internal ref, set its label + properties.
pub fn merge_node(node: &Node) -> CypherStatement {
    let label = sanitize_label(&node.class);
    let mut params: Vec<(String, ParamValue)> =
        vec![("ref".into(), ParamValue::Int(node.id.index() as i64))];

    let mut sets = vec![format!("n.class = ${}", "class")];
    params.push(("class".into(), ParamValue::Text(node.class.to_curie())));

    match &node.source_id {
        Some(sid) => {
            sets.push("n.source_id = $source_id".into());
            params.push(("source_id".into(), ParamValue::Text(sid.to_string())));
        }
        None => {}
    }

    for (i, (key, value)) in node.properties.iter().enumerate() {
        let p = format!("p{i}");
        // property *keys* are inlined (sanitized); values stay parameterized.
        sets.push(format!("n.`{}` = ${}", escape_backtick(key), p));
        params.push((p.clone(), ParamValue::from(value)));
        // A quantity keeps its unit in a companion `<key>__unit` property so the
        // magnitude in Neo4j is never a dimensionless mystery number.
        if let PropertyValue::Quantity { unit, .. } = value {
            let pu = format!("{p}u");
            let ukey = format!("{key}__unit");
            sets.push(format!("n.`{}` = ${}", escape_backtick(&ukey), pu));
            params.push((pu, ParamValue::Text(unit.to_curie())));
        }
    }

    let query = format!(
        "MERGE (n {{_ref: $ref}}) SET n:`{label}`, {}",
        sets.join(", ")
    );
    CypherStatement { query, params }
}

/// `MATCH` both endpoints by ref, `MERGE` the typed relationship.
pub fn merge_edge(edge: &Edge) -> CypherStatement {
    let rel_type = sanitize_rel_type(&edge.relation);
    let query = format!(
        "MATCH (a {{_ref: $from}}), (b {{_ref: $to}}) MERGE (a)-[r:`{rel_type}`]->(b)"
    );
    let params = vec![
        ("from".into(), ParamValue::Int(edge.from.index() as i64)),
        ("to".into(), ParamValue::Int(edge.to.index() as i64)),
    ];
    CypherStatement { query, params }
}

/// Label from a term's local name: `bot:Space` → `Space`. Non-identifier chars
/// are dropped so the result is always a safe backticked label.
fn sanitize_label(term: &Term) -> String {
    let cleaned: String = term
        .name()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if cleaned.is_empty() {
        "Node".into()
    } else {
        cleaned
    }
}

/// Relationship type from a term's local name, SCREAMING_SNAKE by Neo4j
/// convention: `bot:containsElement` → `CONTAINS_ELEMENT`.
fn sanitize_rel_type(term: &Term) -> String {
    let mut out = String::new();
    for (i, ch) in term.name().chars().enumerate() {
        if ch.is_ascii_uppercase() && i != 0 {
            out.push('_');
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else if ch == '_' {
            out.push('_');
        }
    }
    if out.is_empty() {
        "RELATED".into()
    } else {
        out
    }
}

fn escape_backtick(s: &str) -> String {
    s.replace('`', "``")
}

#[cfg(test)]
mod tests {
    use super::*;
    use twin_core::{Properties, SemanticGraph};
    use twin_vocab::bot;

    #[test]
    fn node_merge_is_parameterized_and_labeled() {
        let mut g = SemanticGraph::new();
        let mut props = Properties::new();
        props.insert("name".into(), "Office".into());
        let r = g.add_node(Some("guid-1".into()), bot::SPACE, props);
        let stmt = merge_node(g.node(r).unwrap());
        assert!(stmt.query.contains("n:`Space`"));
        assert!(stmt.query.contains("$ref"));
        assert!(stmt.params.iter().any(|(k, _)| k == "source_id"));
        // value is a param, not inlined
        assert!(!stmt.query.contains("Office"));
    }

    #[test]
    fn quantity_emits_value_and_unit_companion() {
        use twin_core::{PropertyValue, Term};
        let mut g = SemanticGraph::new();
        let mut props = Properties::new();
        props.insert(
            "NetFloorArea".into(),
            PropertyValue::quantity(23.5, Term::owned("unit", "squareMetre")),
        );
        let r = g.add_node(Some("guid-1".into()), bot::SPACE, props);
        let stmt = merge_node(g.node(r).unwrap());
        // magnitude property + its unit companion
        assert!(stmt.query.contains("n.`NetFloorArea` ="));
        assert!(stmt.query.contains("n.`NetFloorArea__unit` ="));
        // magnitude is a Number param, unit is the CURIE as Text
        assert!(stmt
            .params
            .iter()
            .any(|(_, v)| *v == ParamValue::Number(23.5)));
        assert!(stmt
            .params
            .iter()
            .any(|(_, v)| *v == ParamValue::Text("unit:squareMetre".into())));
    }

    #[test]
    fn rel_type_is_screaming_snake() {
        let mut g = SemanticGraph::new();
        let a = g.add_node(None, bot::STOREY, Default::default());
        let b = g.add_node(None, bot::ELEMENT, Default::default());
        g.add_edge(a, b, bot::CONTAINS_ELEMENT);
        let stmt = merge_edge(&g.edges()[0]);
        assert!(stmt.query.contains("`CONTAINS_ELEMENT`"));
    }
}
