# Ket glib security backport

This directory is the crates.io `glib` 0.18.5 release, whose archive SHA-256 is:

```text
233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5
```

Ket vendors it because Tauri 2.11.5's Linux GTK3 stack requires the `glib` 0.18 API, while GHSA-wrw7-89jp-8q8g is fixed upstream only in the incompatible 0.20 release line.

The only source change is the upstream fix from gtk-rs/gtk-rs-core#1343: make the `VariantStrIter::impl_get` out pointer mutable and pass `&mut p` to `g_variant_get_child`. This removes the undefined behavior without changing the GTK3 dependency API.

Remove this patch when the supported Tauri Linux stack no longer resolves a `glib` version affected by GHSA-wrw7-89jp-8q8g. Until then, update the vendored source only from a checksum-verified crates.io archive and reapply the upstream change explicitly.
