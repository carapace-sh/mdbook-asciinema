# mdbook-asciinema

An mdBook preprocessor that embeds [asciinema](https://asciinema.org) terminal recordings.

Markdown image syntax pointing at a `.cast` file is replaced with an embedded
`AsciinemaPlayer`. Configuration can be given between the `[]` brackets:

```md
![120x30@0:05 autoPlay=true speed=2](./demo.cast)
```

- `120x30` sets `cols` x `rows`.
- `@0:05` sets the preview (poster) position.
- Extra `key=value` tokens pass any player option.
- `![](./demo.cast)` uses the cast header's dimensions.

A preview (`poster`) is generated at build time only when a timestamp
is specified (via `@time`, `time=`, `preview=`, `position=`, or `startAt`),
unless an explicit `poster` is given. The cast is not loaded before playing
(`preload` defaults to `false`).

## Setup

1. Build: `cargo build --release` (binary at `target/release/mdbook-asciinema`)
2. Add to `book.toml`:

```toml
[preprocessor.asciinema]
```

3. Place `.cast` files under `src/` and reference them with image syntax.
4. Keep the asciinema player assets in `asciinema/` at the book root.

The preprocessor implicitly adds the asciinema player JavaScript and CSS
to pages that embed a recording, so no `additional-js`/`additional-css`
entries are required.
