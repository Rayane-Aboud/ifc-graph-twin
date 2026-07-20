# Convenience targets. Nothing here is load-bearing — every command is a plain
# `cargo` / `docker compose` invocation you can run by hand.
.PHONY: db db-down db-wipe demo ingest serve test check

# --- Neo4j (docker) ---------------------------------------------------------
db:            ## start local Neo4j in the background
	docker compose up -d

db-down:       ## stop Neo4j, keep the data volume
	docker compose down

db-wipe:       ## stop Neo4j and delete all data
	docker compose down -v

# --- twin CLI ---------------------------------------------------------------
demo:          ## sample graph -> Cypher on stdout (no deps)
	cargo run -p cli -- demo

# Parse a file and stream Cypher (needs the parser). Usage: make ingest FILE=model.ifc
ingest:        ## FILE=path/to/model.ifc — parse, print Cypher
	cargo run -p cli --features twin-ingest-ifc4/real-parser -- ingest $(FILE)

# Parse a file and write it to the running Neo4j. Usage: make persist FILE=model.ifc
persist:       ## FILE=path/to/model.ifc — parse, commit to Neo4j over Bolt
	cargo run -p cli --features full -- ingest $(FILE) --execute

serve:         ## start the read API over an empty graph
	cargo run -p cli -- serve

# --- checks -----------------------------------------------------------------
test:          ## run the workspace tests
	cargo test --workspace

check:         ## typecheck everything, including optional features
	cargo check --workspace --all-features
