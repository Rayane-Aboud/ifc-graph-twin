//! Real IFC ingestion backed by `ifc-lite-core`.
//!
//! Feature-gated (`real-parser`) so the default build stays on `MockSource` with
//! no network dependency. Implements the same [`EntitySource`] trait the mapper
//! already consumes, so nothing in [`crate::mapping`] changes — only the source
//! of the data.
//!
//! ## The one place ifc-lite's API is used
//! [`scan_records`] is the *entire* surface we depend on from `ifc-lite-core`:
//! enumerate `(express_id, type_name, byte_range)` for every entity. Everything
//! else is our own [`crate::steputil`] byte work, unit-tested without ifc-lite.
//! If the crate's scanner signature differs from what's assumed below (it is a
//! fast-moving, thinly-documented crate), this is the only function to adjust —
//! see `TODO(verify-api)`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::source::{EntitySource, IfcEntity, IfcRel};
use crate::{steputil, IngestError};

/// A scanned entity record: its express id, uppercased STEP type keyword, and
/// its top-level attribute fields (already split by [`steputil`]).
struct Record {
    id: u32,
    ty: String,
    fields: Vec<String>,
}

/// The only function that calls into `ifc-lite-core`. Drives the entity scanner
/// over the file bytes and returns one [`Record`] per entity.
///
/// The `ifc-lite-core` surface: `EntityScanner::new(bytes)` +
/// `next_entity() -> Option<(id, type_name, start, end)>`. Verified against
/// v4.1.3 — builds and ingests real STEP files. If a future bump changes the
/// tuple shape or `type_name` type, adjust here only; nothing downstream
/// touches ifc-lite.
fn scan_records(bytes: &[u8]) -> Vec<Record> {
    use ifc_lite_core::EntityScanner;

    let mut scanner = EntityScanner::new(bytes);
    let mut records = Vec::new();
    while let Some((id, type_name, start, end)) = scanner.next_entity() {
        let record = String::from_utf8_lossy(&bytes[start..end]);
        let fields = steputil::attribute_list(&record)
            .map(steputil::split_top_level)
            .unwrap_or_default();
        records.push(Record {
            id,
            ty: type_name.to_string().to_ascii_uppercase(),
            fields,
        });
    }
    tracing::debug!(records = records.len(), bytes = bytes.len(), "scanned STEP entities");
    records
}

/// The four IFC spatial-structure entity keywords.
fn is_spatial(ty: &str) -> bool {
    matches!(
        ty,
        "IFCSITE" | "IFCBUILDING" | "IFCBUILDINGSTOREY" | "IFCSPACE"
    )
}

/// Attribute index of `Name` on any `IfcRoot` subtype (GlobalId, OwnerHistory,
/// Name, ...).
const NAME_ATTR: usize = 2;

/// A relationship expressed in express-id space, before translation to GlobalIds.
struct RawRel {
    relating: u32,
    related: u32,
}

/// An [`EntitySource`] populated from a real IFC file via `ifc-lite-core`.
pub struct IfcLiteSource {
    entities: Vec<IfcEntity>,
    aggregates: Vec<IfcRel>,
    contained: Vec<IfcRel>,
}

impl IfcLiteSource {
    #[tracing::instrument(name = "ifclite_open", skip_all, fields(file = %path.display()))]
    pub fn open(path: &Path) -> Result<Self, IngestError> {
        let bytes = std::fs::read(path).map_err(|source| IngestError::Open {
            path: path.display().to_string(),
            source,
        })?;
        tracing::debug!(bytes = bytes.len(), "read IFC file");
        Ok(Self::from_bytes(&bytes))
    }

    /// Build the store from raw IFC bytes. Separated from [`open`] so it can be
    /// tested against in-memory fixtures.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let records = scan_records(bytes);

        // express id -> GlobalId, for every entity that carries one (field 0
        // being a string literal is exactly the IfcRoot GlobalId).
        let mut id_to_guid: HashMap<u32, String> = HashMap::new();
        for r in &records {
            if let Some(guid) = r.fields.first().and_then(|f| steputil::as_step_string(f)) {
                id_to_guid.insert(r.id, guid);
            }
        }

        // Relationships, in express-id space. Attribute positions differ between
        // the two relation types (see steputil tests): Aggregates is
        // relating-then-related-list; Contained is related-list-then-relating.
        let mut agg_raw = Vec::new();
        let mut cont_raw = Vec::new();
        for r in &records {
            match r.ty.as_str() {
                "IFCRELAGGREGATES" => {
                    if let Some(parent) = r.fields.get(4).and_then(|f| steputil::parse_ref(f)) {
                        for child in r
                            .fields
                            .get(5)
                            .map(|f| steputil::parse_ref_list(f))
                            .unwrap_or_default()
                        {
                            agg_raw.push(RawRel { relating: parent, related: child });
                        }
                    }
                }
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    if let Some(structure) = r.fields.get(5).and_then(|f| steputil::parse_ref(f)) {
                        for element in r
                            .fields
                            .get(4)
                            .map(|f| steputil::parse_ref_list(f))
                            .unwrap_or_default()
                        {
                            cont_raw.push(RawRel { relating: structure, related: element });
                        }
                    }
                }
                _ => {}
            }
        }

        // Keep the graph focused: nodes are the spatial structure plus any
        // element actually placed by a relationship — not every rooted entity
        // (which would drag in property sets, materials, etc.).
        let mut needed: HashSet<u32> = HashSet::new();
        for r in &records {
            if is_spatial(&r.ty) {
                needed.insert(r.id);
            }
        }
        for rr in agg_raw.iter().chain(cont_raw.iter()) {
            needed.insert(rr.relating);
            needed.insert(rr.related);
        }

        let mut entities = Vec::new();
        for r in &records {
            if !needed.contains(&r.id) {
                continue;
            }
            if let Some(guid) = id_to_guid.get(&r.id) {
                let name = r.fields.get(NAME_ATTR).and_then(|f| steputil::as_step_string(f));
                entities.push(IfcEntity {
                    global_id: guid.clone(),
                    ifc_type: r.ty.clone(),
                    name,
                    // TODO(quantities): extract IfcElementQuantity / IfcSIUnit and
                    // populate this so areas/lengths reach the graph with units.
                    quantities: Vec::new(),
                });
            }
        }

        // Translate relationships into GlobalId space, dropping any whose
        // endpoints we didn't ingest.
        let to_guid = |id: u32| id_to_guid.get(&id).cloned();
        let translate = |raws: Vec<RawRel>| -> Vec<IfcRel> {
            raws.into_iter()
                .filter_map(|rr| {
                    Some(IfcRel {
                        relating: to_guid(rr.relating)?,
                        related: to_guid(rr.related)?,
                    })
                })
                .collect()
        };

        Self {
            entities,
            aggregates: translate(agg_raw),
            contained: translate(cont_raw),
        }
    }
}

impl EntitySource for IfcLiteSource {
    fn entities(&self) -> impl Iterator<Item = IfcEntity> + '_ {
        self.entities.iter().cloned()
    }
    fn aggregates(&self) -> impl Iterator<Item = IfcRel> + '_ {
        self.aggregates.iter().cloned()
    }
    fn contained_in_spatial(&self) -> impl Iterator<Item = IfcRel> + '_ {
        self.contained.iter().cloned()
    }
}
