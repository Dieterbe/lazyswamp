# Lazyswamp

Lazyswamp is a terminal interface for [Swamp](https://swamp.club). It lets you
browse managed models, run their methods through schema-derived forms, and
inspect the versioned data those methods produce.

## Features

- Search models and inspect their definitions, types, methods, arguments, and
  output specifications.
- Validate, review, run, monitor, and cancel model methods. Every run requires
  review; destructive-looking methods also require the model name.
- Browse artifact metadata and content, select historical versions, and compare
  JSON structurally or UTF-8 text line by line.

Lazyswamp is deliberately read-only apart from running and cancelling model
methods. It does not create models or rename, prune, garbage-collect, or delete
data.

## Requirements and installation

- Rust 1.85 or newer
- `swamp` installed and available on `PATH`
- An initialized local Swamp repository

Install from a clone:

```sh
cargo install --path .
```

Run Lazyswamp inside a Swamp repository:

```sh
lazyswamp
```

Or select the repository and CLI explicitly:

```sh
lazyswamp --repo-dir /path/to/repo --swamp-bin /path/to/swamp
```

## Navigation

| Key | Action |
| --- | --- |
| Arrow keys or `j`/`k` | Move the active selection |
| `Tab` | Switch between model and content focus |
| `1`, `2`, `3` | Open Overview, Methods, or Data |
| `Enter` | Open, load, validate, or confirm |
| `/` | Filter models by name or type |
| `r` | Refresh the current view |
| `[` / `]` | Select a data version |
| `a` / `b` | Mark the two versions to compare |
| `c` | Cancel the active method run |
| `?` | Open contextual help |
| `Esc` | Close a dialog |
| `q` | Quit |

Method inputs support common JSON Schema types and constraints. Schemas using
references, composition, or conditionals fall back to a raw JSON editor. Fields
marked `writeOnly` or `format: password` are masked and redacted from the review
screen. Inputs are held only in memory and sent to Swamp as JSON over stdin.

Content larger than 1 MiB requires confirmation before loading. Binary content
shows metadata but is not rendered or compared.

## Design decisions

Lazyswamp uses Rust and Ratatui because they provide a portable single binary,
typed response handling, and mature terminal widgets. Go with Bubble Tea and
TypeScript with Ink were considered; both are viable, but Rust best fits the
single-binary and strongly typed CLI-adapter goals selected for this project.

All operations go through the supported `swamp` CLI with JSON retrieval output.
This follows Swamp's architecture and data guidance: models own typed actions,
while data is queried through Swamp's versioned catalog. Reading `.swamp/`
directly was rejected because it would bypass datastore abstractions and couple
the UI to private storage layouts.

The first release supports local repositories only. Remote `swamp serve`
connections, workflows, reports, model creation, a complete JSON Schema UI,
concurrent runs, crates.io publication, and prebuilt release binaries were
considered but deferred to keep the initial safety and browsing flows focused.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
```

Tests use client doubles and checked-in response fixtures; they do not require
network access or a live Swamp repository.

Lazyswamp is licensed under the [MIT License](LICENSE).
