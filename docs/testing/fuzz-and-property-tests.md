# Fuzz and property tests

Two suites keep the API honest about untrusted input: a **property suite** that runs inside
`cargo test` on every build, and a pair of **fuzz targets** that explore the same contracts with a
coverage-guided mutator when a contributor asks for them. Neither one asserts a fixed expected
value; both assert a *contract* — a decode answers or errors, never panics, and a sanitised
identifier never becomes a path that leaves the data root.

- **Property suite:** [`tests/property.rs`](../../tests/property.rs) — issue
  [#100](https://github.com/TucanoTechnology/TucanoTestAPI/issues/100)
- **Fuzz crate:** [`fuzz/`](../../fuzz/) — issue
  [#100](https://github.com/TucanoTechnology/TucanoTestAPI/issues/100)
- **Parent epic:** [#16](https://github.com/TucanoTechnology/TucanoTestAPI/issues/16)

## 1. What runs where

| Half | Runner | When it runs | What it is for |
| --- | --- | --- | --- |
| Property suite | `cargo test` (proptest) | Every build, every CI run | Bounded, generated inputs for every decode and sanitisation entry point |
| Fuzz targets | `cargo fuzz` (libFuzzer) | On demand, by a contributor | Coverage-guided, unbounded search for the input that breaks a contract |

The split follows one rule: **nothing long-running runs in CI.** The property suite is deliberately
cheap — `ProptestConfig::with_cases(128)` and inputs bounded by construction — so it can ride along
with every `cargo test`. Fuzzing is a tool a contributor reaches for when changing a decoder or a
path builder, not a gate that every pull request waits behind.

## 2. The property suite

```sh
cargo test --test property
```

Every property in the file is generated from one list of document types, held in the
`for_every_model!` macro at the top of [`tests/property.rs`](../../tests/property.rs). Adding a model
to that list widens all of them at once:

- `every_model_decodes_arbitrary_text_without_panicking` — `serde_json::from_str`;
- `every_model_decodes_arbitrary_bytes_without_panicking` — `serde_json::from_slice`, with the bytes
  not assumed to be UTF-8;
- `every_model_decodes_arbitrary_json_values_without_panicking` — `serde_json::from_value`, with
  object keys drawn from the models' own camelCase field names so the generated document reaches the
  field decoders instead of stopping at `deny_unknown_fields`;
- `every_model_decodes_mutations_of_valid_documents_without_panicking` — every valid sample with one
  byte-level mutation, which lands in the decoders far more often than pure noise does.

The samples the mutation property mutates are pinned by two plain `#[test]`s, so the seed corpus
cannot rot: `every_sample_decodes_as_its_model` decodes each one as the type it is labelled with, and
`the_samples_cover_exactly_the_models_this_suite_decodes` requires the sample table and the model
list to name exactly the same set.

The same file pins the parsers that turn request bodies and imported reports into documents — the
JUnit reader, the JSON import reader, payload validation, the defect-link request parser, the defect
URL validator, and the summary-report date filter — with the same no-panic contract.

The sanitisation properties pin what the layout actually guarantees rather than a looser reading of
it:

- `validate_component_accepts_exactly_single_plain_components` — a value is accepted exactly when it
  is a non-empty name that is neither `.` nor `..` and carries no separator or NUL;
- `node_folder_accepts_exactly_the_identifiers_the_layout_can_store` and
  `validate_document_id_accepts_exactly_a_component_with_its_suffix` — the per-resource rules above
  the component rule;
- `a_wire_id_round_trips_through_its_folder_name` — `folder_name(&folder_wire_id(f)) == f` for every
  string `f`, so no identifier is lost or reshaped on the way to disk;
- `every_path_builder_stays_inside_the_data_root` — for every builder and both parent shapes, an
  accepted path starts with the data root, passes `ensure_within`, is made only of `Component::Normal`
  below it, sits at its documented depth, and (for attachments) ends with the supplied file name;
  every refusal is `io::ErrorKind::InvalidInput`, never a permissions error from a path that escaped.

The attachment-name helpers have their own group, because they are the one place a caller-supplied
string is replayed on the wire: a plain stored name needs no RFC 5987 form, a `Content-Disposition`
value is always a safe ASCII header value, a generated stored name recovers its original, a recovered
name is always a tail of the stored name, and the recorded media type depends only on the lowercased
extension.

## 3. The fuzz targets

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
cd fuzz
cargo +nightly fuzz run deserialize_models
cargo +nightly fuzz run sanitise_identifiers
```

`cargo fuzz` needs a nightly toolchain and the `cargo-fuzz` subcommand; neither is part of the build,
so install them only when you intend to fuzz. Both targets live in the crate at
[`fuzz/`](../../fuzz/):

- `deserialize_models` — reads arbitrary bytes as every document type and only ever asserts that a
  decode answered (`Ok` or `Err`). It is the coverage-guided counterpart of the four decode
  properties.
- `sanitise_identifiers` — reads arbitrary bytes as a UTF-8 identifier and pushes it through
  `validate_component`, `node_folder`, `validate_document_id`, the wire-id round trip and every path
  builder, asserting two things: anything accepted is one plain path element, and every path built
  from it stays inside the data root.

A short run is enough to confirm a change:

```sh
cargo +nightly fuzz run deserialize_models -- -runs=2000
```

`fuzz/Cargo.toml` declares its own `[workspace] members = ["."]`, which keeps the fuzz crate out of
the root `cargo clippy --all-targets --all-features` and `cargo fmt --all` runs: a `#![no_main]`
target is not something the stable build gate should see. The price of that separation is that the
fuzz targets can drift away from the suite's model list, so a plain `#[test]` pays it —
`the_fuzz_target_decodes_every_model_this_suite_decodes` reads
[`fuzz/fuzz_targets/deserialize_models.rs`](../../fuzz/fuzz_targets/deserialize_models.rs) and fails
`cargo test` if the target does not mention every model the suite decodes. Adding a model to
`for_every_model!` therefore means adding it to the fuzz target in the same commit.

## 4. Following up

If a property or a fuzz target finds a real defect, the fix belongs in the production code and the
finding belongs in the suite. Proptest writes a `PROPTEST_REGRESSIONS` file (here
`tests/property.proptest-regressions`) recording a failing case; keep it and make the fixed case
permanent. A fuzz crash is written under `fuzz/artifacts/`, which is ignored by git — copy the input
into the property suite as a sample or a regression before closing the issue, so the contract keeps
being checked after the local artifact is gone.
