# SoDeck GLib compatibility backport

This directory preserves the published `glib` 0.18.5 crate, including its MIT license and copyright notices. The archive SHA-256 is `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`; its upstream Git revision is `42b9caf98e03ded086362d9653ca58fe94dc8658`.

The only upstream source change is the two-line `VariantStrIter::impl_get` fix from [gtk-rs/gtk-rs-core#1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343), merged as `05dff0ee696f9bcd8617cd48c4b812d046d440cb`: make the output pointer mutable and pass `&mut p` to `g_variant_get_child`. This addresses [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html).

GTK3 and Tauri currently require the 0.18 API. The crate retains its real upstream version; it is not relabeled as a newer release. The core's `[patch.crates-io]` entry applies this source to both direct and transitive uses of `glib` 0.18.5. A direct 0.20 dependency would leave the affected GTK dependency in place.

SoDeck adds this provenance note, a locked test dependency graph, and `tests/variant_str_iter.rs`. The upstream README's Rustdoc `struct@Variant` link is expanded to its docs.rs URL so it also works as repository Markdown. Run `npm run glib:test` on Linux to exercise forward, reverse and mixed string iteration with release optimizations. A normal core build compiles the same patched source.

Remove this override when the supported Tauri/GTK stack can use an upstream release containing the fix, then regenerate the core lockfile and repeat Linux regression and native package validation. Until then, maintain this backport explicitly; an automated glib version bump is not sufficient. GitHub's version-based advisory matching may still require review even though the source is patched.
