# ifc-graph-twin

ifc-graph-twin is the base layer for a digital twin, written in Rust. It reads building models (IFC4
files), turns them into one neutral graph, stores that graph in Neo4j and answers queries about it
through a small API.

I think the hard part of digital twins is often not the AI or the physics, but getting data from
different tools into one place and querying it quickly. So this project focuses on that part: reading,
storing and querying. Machine learning or simulation can be built on top later.

## How it's organised

Everything goes through one neutral model in `core`. Code that is specific to a file format or to a
database stays at the edges and never reaches the middle.

```
 input format              neutral core                 graph backend
┌────────────────┐      ┌────────────────┐         ┌────────────────┐
│ ingest-ifc4    │─────▶│  SemanticGraph │────────▶│ store-neo4j    │
│ (ingest-*)     │      │  Term (+vocab) │         │ (store-*)      │
└────────────────┘      └───────┬────────┘         └────────────────┘
                                │
                          ┌─────▼─────┐
                          │    api    │  read-only queries over the graph
                          └───────────┘
```

The project can grow in three directions, and each one has a clear place for new code:

| Direction | Where new code goes | Examples for later |
|---|---|---|
| A new input format | a new `crates/ingest-*` crate | `ingest-citygml`, `ingest-ifc2x3` |
| A new graph database | a new `crates/store-*` crate | `store-rdf`, `store-memgraph` |
| A new ontology | a new module in `crates/vocab` (`twin-vocab`) | Brick, SAREF, QUDT |

For now the input is IFC4 (the ISO 16739 standard) and the vocabulary is BOT, the Building Topology
Ontology from the W3C Linked Building Data group. Neither is hard-coded in the core.

### One shared vocabulary

With several formats and several ontologies, you could end up writing a mapping for every pair (IFC to
BOT, IFC to Brick, CityGML to BOT...). I avoided that on purpose. Every `ingest-*` crate translates into
one shared vocabulary, BOT, so the graph looks the same whatever the source file was. If another ontology
is needed later, it will be a translation of the finished graph, not a new input path. That way the
amount of work grows by adding, not by multiplying.

### Why there are no traits yet

Right now there is one parser, one database and one ontology. That's too few to know what a good
`Parser` or `Store` interface should look like, so I didn't write one yet. The crate boundaries play
that role for now. When a second `ingest-*` or `store-*` crate arrives, extracting a trait from the
existing functions will be a simple refactor, because everything already goes through `SemanticGraph`.

The one place where I did plan for extension is the vocabulary. Node classes and edge types are open,
namespaced `Term`s (like `bot:Space`) instead of a fixed enum. The `Term` type lives in `core`, and the
known constants live in a separate `twin-vocab` crate, so `core` doesn't depend on any ontology. Adding
Brick or SAREF terms means adding a module to `twin-vocab`, without touching any other crate.

## The crates

- **`core`** (`twin-core`): the neutral graph, with `SemanticGraph`, `Node`, `Edge`, the open `Term` type
  and the `SourceId` / `NodeRef` identities. It knows nothing about ontologies, file formats or
  databases. Node values are `PropertyValue`s (text, number, boolean, or a `Quantity { value, unit }`, so
  IFC measurements keep their units). The unit is also an open `Term`, so `unit:squareMetre` works today
  and QUDT units can come later. Grouped IFC property sets (Psets) will come later, when a real use needs
  them.
- **`vocab`** (`twin-vocab`): named constants for the BOT vocabulary, and later for other ontologies. It
  only depends on `core`.
- **`ingest-ifc4`** (`twin-ingest-ifc4`): reads IFC4 into a `SemanticGraph` using BOT. The mapping rules
  are in `mapping.rs`, and entities come from an `EntitySource`. Real files are read with `IfcLiteSource`,
  based on `ifc-lite-core`, behind the `real-parser` feature. Without that feature, `ingest` stops with an
  error instead of making up data. `MockSource` is only used for tests and for `twin demo`.
- **`store-neo4j`** (`twin-store-neo4j`): writes a `SemanticGraph` to Neo4j with parameterised Cypher
  queries that are safe to run twice. `write_graph` sends the statements to an output without any network
  (the default), and `execute_graph` runs the same statements in a single Bolt transaction with the `bolt`
  feature (`neo4rs`).
- **`api`** (`twin-api`): a small Axum server that reads a `SemanticGraph`. Its endpoints are `/health`,
  `/stats` (counts by class and by source model), `/nodes` (filter by `class` and `model`), `/nodes/{id}`
  and `/nodes/{id}/neighbors` (the direct neighbours of a node). It doesn't depend on any `ingest-*` crate.
- **`cli`**: puts it all together, with `ingest <file>` and `serve`.

A note on names: the `store-*` crates are the databases the graph is written to. Where the data comes
*from* is called an `EntitySource`, inside `ingest-*`, so the two are not confused.

### Streaming at the edges, the full graph in the middle

Both ends of the pipeline stream. `EntitySource` gives entities one by one through iterators, so a future
parser could read them straight from the file without keeping a big list in memory. On the other side,
`write_graph` sends each Cypher statement to an output as soon as it's ready, so statements can be
printed, counted or run one at a time.

The graph in the middle, however, has to be fully in memory. In IFC, entities refer to each other by
`GlobalId`, and a relationship can point to an entity that appears later in the file. So all the entities
are loaded into the `SemanticGraph` first, and the relationships are linked after that.

### Writing and reading use the same graph

Saving the graph (`ingest` to Neo4j) and querying it (`serve` over HTTP) are two separate uses of it. The
risk would be to build the graph twice and end up with two versions that differ. To avoid that, the graph
is built in one place (`load_graph` in the CLI) and the same object is used for both. With
`serve --neo4j-uri`, the graph that is saved is exactly the one being served.

## Usage

```bash
# demo with no dependencies, using the built-in sample graph
cargo run -p cli -- demo                           # sample -> graph -> Cypher
cargo run -p cli -- serve --addr 127.0.0.1:8080    # query API over an empty graph

# reading real files needs the parser feature; without it, `ingest` stops with an error
cargo run -p cli --features twin-ingest-ifc4/real-parser -- ingest path/to/model.ifc

# serve the graph and print its Cypher in the same run
cargo run -p cli --features twin-ingest-ifc4/real-parser -- \
  serve --file path/to/model.ifc --neo4j-uri bolt://127.0.0.1:7687
```

### Saving to a real Neo4j (Docker)

The commands above only print the Cypher. To really write to a database, start the Neo4j container and
run `ingest` with `--execute` (feature `bolt`):

```bash
docker compose up -d          # Neo4j at bolt://127.0.0.1:7687, web UI on :7474 (neo4j/password)

# read a file and save it over Bolt, in one transaction
cargo run -p cli --features full -- ingest path/to/model.ifc --execute

docker compose down           # stop it (add -v to delete the data)
```

`--features full` turns on both optional parts (`bolt` and `real-parser`). Use `--features bolt` alone if
you only need the database driver. The login matches the compose file (`neo4j` / `password`) and can be
changed with `--neo4j-user` / `--neo4j-password`. The `Makefile` has shortcuts (`make db`,
`make persist FILE=...`). Without the `bolt` feature, `--execute` stops with an error instead of only
printing.

### Querying several models together

A real project usually has several models of the same building, one per discipline (architecture,
structure, HVAC...). You can pass `--file` several times to merge them into one graph and query across
them. Each file's nodes get a `model` label from the file name, so you can ask questions per discipline,
which a normal viewer can't do.

```bash
cargo run -p cli --features twin-ingest-ifc4/real-parser -- serve \
  --file "Sample-Test-Files/IFC 4.0.2.1 (IFC 4)/PCERT-Sample-Scene/Building-Architecture.ifc" \
  --file "Sample-Test-Files/IFC 4.0.2.1 (IFC 4)/PCERT-Sample-Scene/Building-Structural.ifc" \
  --file "Sample-Test-Files/IFC 4.0.2.1 (IFC 4)/PCERT-Sample-Scene/Building-Hvac.ifc"
```

Use your real paths. The folder names contain spaces, so keep the quotes. Then, on
`http://127.0.0.1:8080`:

```bash
# how much each discipline added to the merged graph
curl 'localhost:8080/stats'
# -> { nodes, edges, by_class: {"bot:Space":2,...}, by_model: {"architecture":N,"structural":M,"hvac":K} }

curl 'localhost:8080/nodes?class=Space'                     # all spaces, from any discipline
curl 'localhost:8080/nodes?class=Element&model=structural'  # only the structural elements
curl 'localhost:8080/nodes/{globalId}/neighbors'            # what a storey contains, or a building's spaces
```

This is where the graph is useful: "elements per discipline" or "what contains what" are one request
each, because relationships are stored directly instead of being rebuilt by reading the IFC file.

## Next steps

1. ~~Use a real IFC parser instead of `MockSource`.~~ Done: `ifc-lite-core` behind the `real-parser`
   feature (`ingest-ifc4/src/ifclite.rs`).
2. ~~Connect a Neo4j driver in `store-neo4j`.~~ Done: `execute_graph` with `neo4rs` behind the `bolt`
   feature, and `docker compose up` for a local database.
3. Run a complete sample IFC file from start to end against a local Neo4j.
4. Compare parsing and loading speed with IfcOpenShell.
