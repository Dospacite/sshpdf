# Vendored crates

## hayro (0.9.0, Apache-2.0 OR MIT)

`vendor/hayro` is the `hayro` crate 0.9.0 from crates.io
(<https://github.com/LaurenzV/hayro>), unchanged except for:

- `src/lib.rs`: the block between `--- sshpdf patch` and `--- end of sshpdf
  patch` adds the `RenderFilter` trait and `render_into_filtered`, plus a
  `use std::borrow::Cow;` import.
- `Cargo.toml`: the `[[example]]` entry was removed (the example was not
  copied); `Cargo.lock`, `.cargo-ok` and `.cargo_vcs_info.json` were removed.

Why: sshpdf draws text and table rules with terminal characters and must
render the rest of the page *without* them (`Doc::render` with
`Exclusions`). hayro's renderer device is crate-private, so nothing outside
the crate can filter what it draws. `render_into_filtered` wraps the renderer
in a device that asks a `RenderFilter` about every glyph run and path first.

Updating: copy the new hayro release over this directory, re-apply the block
above (it only uses public hayro-interpret types plus `Renderer::new` and
`GlobalState::new`), and bump the `hayro-interpret` version to match. The
patch is small enough to upstream; once hayro exposes a hook like it, switch
the workspace dependency back to crates.io and delete this directory.

The license files of the original crate are kept next to it.
