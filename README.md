# ifc-graph-twin

A fast ingestion + query core for digital twins: parse building/city models into
a neutral **semantic graph**, store it, and serve live queries over it. Both
edges stream (see "Streaming seams, resident graph"); the canonical graph in the
middle is held resident.

The bet: the bottleneck in digital twins isn't AI or physics — it's
interoperability and real-time infrastructure. This project owns the
**ingestion + storage + query** backbone so the ML/physics layers can build on top.

## Architecture

Everything flows through one neutral model in `core`. Format-specific and
backend-specific code lives at the edges and never leaks into the middle.

```
 input notation            neutral core                 graph backend
┌────────────────┐      ┌────────────────┐         ┌────────────────┐
│ ingest-ifc4    │─────▶│  SemanticGraph │────────▶│ store-neo4j    │
│ (ingest-*)     │      │  Term (+vocab) │         │ (store-*)      │
└────────────────┘      └───────┬────────┘         └────────────────┘
                                │
                          ┌─────▼─────┐
                          │    api    │  read-only queries over the graph
                          └───────────┘
```

### The scaling axes are encoded in the crate names

This project is designed to grow along three independent axes. Each axis has an
obvious, mechanical place to add code — no rewrites, no guessing:

| Axis | Where new work goes | Examples to come |
|------|---------------------|------------------|
| **Input notation** | a new `crates/ingest-*` crate | `ingest-citygml`, `ingest-ifc2x3` |
| **Graph backend** | a new `crates/store-*` crate | `store-rdf`, `store-memgraph` |
| **Ontology / vocabulary** | a new module in `crates/vocab` (`twin-vocab`) | Brick, SAREF, QUDT |

Today's defaults are **IFC4** (input, per ISO 16739) and **BOT** (vocabulary,
the W3C Linked Building Data group's Building Topology Ontology). Neither is
hardcoded into the core — see below.

### Canonical vocabulary: keep it N×1, not N×M

Notations and ontologies could in principle multiply into an N×M matrix of
mappings (IFC→BOT, IFC→Brick, CityGML→BOT, …). We deliberately avoid that. Every
`ingest-*` crate normalizes into **one canonical vocabulary — BOT** — so the
graph reads the same regardless of source format. Expressing the graph in another
ontology is then a *downstream projection* over the finished canonical graph, not
a parallel ingest target. That keeps the work at N ingesters × 1 canonical form,
plus M projections — additive on each axis instead of multiplicative. Those
projections get built when a second ontology actually lands, not before.

### Why there are no traits yet

We deliberately have **one** parser, **one** backend, **one** default ontology.
By the rule of three, that's too few to justify abstract interfaces — extracting
a `Parser`/`Store` trait now would be guessing at a shape we haven't proven.

Instead, the *crate boundaries* are the seam. When a second `ingest-*` or
`store-*` crate arrives, pulling a trait out of the existing concrete functions
is a mechanical refactor, because everything already passes through
`SemanticGraph`.

The one place we **do** design for extension up front is the vocabulary: node
classes and edge relations are open, namespaced `Term`s (CURIE-style, e.g.
`bot:Space`), not a closed enum. The `Term` *mechanism* lives in `core`; the
*known constants* live in a separate `twin-vocab` crate, so `core` itself stays
free of any specific ontology. Adding Brick or SAREF terms is a new module in
`twin-vocab` and touches no other crate. That's a data-representation choice, not
an abstraction layer — so it doesn't violate the rule above.

## Crates

- **`core`** (`twin-core`) — the neutral semantic graph. `SemanticGraph`, `Node`,
  `Edge`, the open `Term` type, `SourceId`/`NodeRef` identities. Knows no
  ontology, no notation, no backend. The load-bearing seam. Node values are
  `PropertyValue` (text/number/bool, plus a unit-bearing `Quantity { value, unit
  }` so IFC magnitudes don't reach the graph dimensionless); the unit is an open
  `Term`, so `unit:squareMetre` today and QUDT later cost nothing. Grouped IFC
  property sets (Psets) are a deliberate later step, gated on a real producer.
- **`vocab`** (`twin-vocab`) — named constants for the canonical BOT vocabulary
  (and future ontologies). Depends only on `core`. Kept separate so `core` is
  genuinely ontology-free.
- **`ingest-ifc4`** (`twin-ingest-ifc4`) — concrete IFC4 → `SemanticGraph`,
  emitting BOT. Mapping rules live in `mapping.rs`. The entity feed is an
  `EntitySource`. File ingestion uses the real `ifc-lite-core`-backed
  `IfcLiteSource` under the `real-parser` feature; without it, `ingest` fails
  loudly rather than fabricating data. `MockSource` is a pure test/demo fixture
  (reached via `twin demo`), never wired into file parsing.
- **`store-neo4j`** (`twin-store-neo4j`) — concrete `SemanticGraph` → Neo4j.
  Idempotent, parameterized Cypher. Two consumers share one set of statement
  builders: `write_graph` streams statements to a sink offline (default, no
  network), and `execute_graph` runs the same statements in a single Bolt
  transaction under the `bolt` feature (`neo4rs`).
- **`api`** (`twin-api`) — thin Axum read layer over a `SemanticGraph`.
  Endpoints: `/health`, `/stats` (counts by class and by source model),
  `/nodes` (filterable by `class` and `model`), `/nodes/{id}`, and
  `/nodes/{id}/neighbors` (one-hop graph traversal). Ontology-neutral: it reads
  classes and the `model` provenance tag as plain data, with no dependency on any
  `ingest-*` crate.
- **`cli`** — wires it together: `ingest <file>` and `serve`.

Note on the word "store": `store-*` crates are graph **backends** the pipeline
writes to. The read side — where entities come *from* — is an `EntitySource`
inside `ingest-*`, deliberately named to avoid the collision.

### Streaming seams, resident graph

Both edges of the pipeline are streaming seams. `EntitySource` yields **owned
items through iterators**, not borrowed slices, so a future lazy parser can emit
entities straight off the byte stream with no backing collection — the mapper
never learns whether it got a `Vec` or a live stream. On the write side,
`write_graph` pushes each `CypherStatement` to a **sink** (`FnMut(CypherStatement)
-> Result<..>`) instead of returning a `Vec`, so statements can be printed,
counted, or run in a Bolt transaction one at a time without buffering the batch.

The middle stays resident, and that is fundamental, not laziness. IFC relates
entities by `GlobalId`, and those references are forward-declarable: a
relationship can name a node that appears later in the file. So the mapper drains
all entities into the `SemanticGraph` first, then resolves relationships against
it. The graph must be fully present before edges can be linked. Streaming applies
to the *feed in* and the *emit out*; the canonical graph itself is held in memory
by necessity.

### Write model and read model, one build path

Persisting (`ingest` → Neo4j) and querying (`serve` → HTTP) are two projections
of the graph — a CQRS-shaped split. The trap would be to build them separately:
persist one file, serve another, and let the two drift. We avoid that by building
the graph through **one** path (`load_graph` in the CLI) and handing that exact
instance to both sides. `serve --neo4j-uri` write-throughs the same in-memory
`SemanticGraph` it is about to serve, so within a run the persisted model and the
queried model are one object, not two builds. No read trait is introduced —
there is still only one backing store — but the seam is honest.

## Usage

```bash
# zero-dependency demo — the built-in sample graph, clearly labeled
cargo run -p cli -- demo                           # sample -> graph -> Cypher
cargo run -p cli -- serve --addr 127.0.0.1:8080    # query API over an (empty) graph

# real ingestion needs the parser feature; without it, `ingest` fails loudly
cargo run -p cli --features twin-ingest-ifc4/real-parser -- ingest path/to/model.ifc

# serve AND emit the same graph's Cypher in one run (write model == read model)
cargo run -p cli --features twin-ingest-ifc4/real-parser -- \
  serve --file path/to/model.ifc --neo4j-uri bolt://127.0.0.1:7687
```

### Persisting to a live Neo4j (Docker)

The offline paths above only *print* Cypher. To actually write to a database,
start the bundled Neo4j and ingest with `--execute` (feature `bolt`):

```bash
docker compose up -d          # Neo4j at bolt://127.0.0.1:7687, UI on :7474 (neo4j/password)

# parse a file and commit it over Bolt, atomically, in one transaction
cargo run -p cli --features full -- ingest path/to/model.ifc --execute

docker compose down           # stop (add -v to wipe the data volume)
```

`--features full` turns on both optional backends (`bolt` + `real-parser`); use
`--features bolt` alone if you only need the driver. Auth defaults match the
compose file (`neo4j` / `password`) and can be overridden with `--neo4j-user` /
`--neo4j-password`. A `Makefile` wraps these (`make db`, `make persist FILE=...`).
Without the `bolt` feature, `--execute` fails loudly rather than silently
printing — the same fail-loud stance `ingest` takes without `real-parser`.

### Federated queries: merge disciplines, ask across them

A real project is not one IFC file — it's several discipline models (architecture,
structure, HVAC, ...) of the same building. Repeat `--file` to merge them into
**one** graph and query across the seam. Each file's nodes are tagged with a
`model` label derived from its name, so you can ask discipline-scoped questions a
viewer can't.

```bash
cargo run -p cli --features twin-ingest-ifc4/real-parser -- serve \
  --file "Sample-Test-Files/IFC 4.0.2.1 (IFC 4)/PCERT-Sample-Scene/Building-Architecture.ifc" \
  --file "Sample-Test-Files/IFC 4.0.2.1 (IFC 4)/PCERT-Sample-Scene/Building-Structural.ifc" \
  --file "Sample-Test-Files/IFC 4.0.2.1 (IFC 4)/PCERT-Sample-Scene/Building-Hvac.ifc"
```

(Use the real paths, not `...` — the folder names contain spaces, so keep the
quotes.) Then, against `http://127.0.0.1:8080`:

```bash
# how much did each discipline contribute to the merged graph? (the federation proof)
curl 'localhost:8080/stats'
# -> { nodes, edges, by_class: {"bot:Space":2,...}, by_model: {"architecture":N,"structural":M,"hvac":K} }

curl 'localhost:8080/nodes?class=Space'                 # all spaces, any discipline
curl 'localhost:8080/nodes?class=Element&model=structural'  # only structural elements
curl 'localhost:8080/nodes/{globalId}/neighbors'        # what a storey contains / a building's spaces
```

The graph's payoff: "elements per discipline" and "what contains what" are one
request each, because relationships are first-class — not something to reconstruct
by walking an IFC file.

## Next steps

1. ~~Pin a real IFC parser and replace `MockSource`.~~ Done: `ifc-lite-core`
   behind the `real-parser` feature (`ingest-ifc4/src/ifclite.rs`).
2. ~~Wire a Neo4j driver in `store-neo4j`.~~ Done: `execute_graph` over `neo4rs`
   behind the `bolt` feature, with `docker compose up` for a local instance.
3. Run a full-hierarchy sample IFC file end to end against a local Neo4j.
4. Benchmark parse+ingest throughput against IfcOpenShell — the credibility story.
