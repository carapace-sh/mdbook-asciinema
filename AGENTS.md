# AGENTS.md

## Project

`mdbook-asciinema` is an mdBook preprocessor (Rust binary) that replaces
markdown image links to `.cast` files with embedded asciinema players. Single
crate, single source file: `src/main.rs`. No tests exist.

## Commands

- Build (dev): `cargo build`
- Build (release): `cargo build --release` (binary `target/release/mdbook-asciinema`)
- Test: `cargo test` (currently no tests)
- Lint/clippy: not configured in CI
- Run as preprocessor: invoked by mdBook via binary name `mdbook-asciinema`; reads JSON on stdin, writes JSON on stdout. The `supports` subcommand exits 0.

## Architecture

mdBook invokes the binary as a preprocessor. `main()` reads the mdBook context
array from stdin (`[ctx, book]`), extracts `root`, `config.book.src`, and the
`preprocessor.asciinema` config (`version`, `download`), optionally downloads
player assets, then recursively walks `book.items` → `Chapter` → `sub_items`,
transforming each chapter's `content`.

`transform_content` regex-matches `![alt](path.cast)` (optional title) and
replaces with a `<div>` + `<script>` calling `AsciinemaPlayer.create`. It also
prepends `<link>`/`<script>` tags for the player CSS/JS once per changed page,
with relative prefix derived from the chapter's `source_path` depth.

`ensure_assets` downloads `asciinema-player.min.js` and `asciinema-player.css`
from the asciinema-player GitHub releases into `<root>/<src>/asciinema/` (only
missing files), using `curl`. `download` defaults to true; `version` selects a
release tag (`latest` or `vX`).

## Config (book.toml)

```toml
[preprocessor.asciinema]
version = "..."   # optional release tag
download = true    # default true
```

## Cast parsing

`parse_cast` reads asciicast v2: first non-empty line is JSON header with
`term.cols`/`term.rows` (or top-level `cols`/`rows`); subsequent lines are
`[interval, code, data]` arrays. `generate_preview` accumulates `"o"` events
until the target time to build a poster string.

## Conventions / gotchas

- Version is `0.0.0-PLACEHOLDER` in `Cargo.toml`/`Cargo.lock`; CI substitutes
  the tag version via `sed` and marks files `--assume-unchanged`. Do not
  hardcode a version.
- Asset download requires `curl` on PATH and network access; `ensure_assets`
  returns early if `root` is empty. Files are only downloaded if absent, so
  stale assets won't refresh.
- Unique element ids come from a process-global `AtomicUsize` counter.
- `cols`/`rows` default to 108/24 when the cast header lacks them.
- Preview (poster) is generated only when a timestamp is given (`@time`,
  `time=`, `preview=`, `position=`, or `startAt`) and no explicit `poster`.
- `parse_time` supports colon-separated `mm:ss` / `h:mm:ss` (rightmost field
  seconds, each leftward ×60).
- Output JSON is compact (`serde_json::to_string`).
- No test suite; verify changes by building a sample mdBook (see README) and
  inspecting generated HTML.
- Goreleaser targets only `x86_64-unknown-linux-gnu`.
