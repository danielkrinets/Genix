# Genix — continuation guide

## Product and implementation

Native parametric CAD built in Rust, egui 0.31 / eframe / wgpu. Do not replace it with a web app. User-facing copy defaults to Russian; English and Hebrew dictionaries are in `crates/app/src/i18n_*.rs`.

The product path is local mock registration → model library with real geometry previews → CAD editor. Profile and server subscription are **local mocks**: no backend, payment or password persistence. Release checks use the public GitHub API. Keep those limits explicit.

## Source map

- `crates/core/src/doc.rs`: document/evaluator, features, history, `EdgeRef` serialization, per-body materials and transparent cutter-body state.
- `crates/core/src/geom.rs`: geometric tools, including bounded circular-arc fillets/chamfers.
- `crates/app/src/app.rs`: app lifecycle, tabs, history, selection, viewport, hole center/axis snapping, settings.
- `crates/app/src/edges.rs`: mesh sharp-edge recovery into lines, full circles and **open arcs**. Never fit partial arcs by averaging their vertices or silently revolve a full 360° tool for them.
- `crates/app/src/commands.rs`: operation properties, hole center button and placement grid.
- `crates/app/src/desktop.rs`: document request queue, native macOS menus, open-document AppleEvent handler and Finder reveal. **Do not replace Winit's NSApplication delegate**: the installed Winit version requires its own delegate and aborts if replaced.
- `crates/app/src/home.rs`: account gate, library/search/filters, auto-hiding navigation, dedicated settings, profile and subscription mocks.
- `crates/app/src/widgets.rs`: shared checkboxes/switches, feedback and accessibility semantics.
- `crates/app/src/backgrounds.rs`: native adaptations of React Bits waves; passive mouse light and bounded, decaying ripples, reduced-motion and touch handling.
- `crates/app/src/files.rs`: system dialogs and atomic writes. Preserve old data on write failure.
- `packaging/macos/Info.plist`: `.genix` / `.ostov` document association; `bundle.sh` produces ad-hoc-signed local ZIPs. No Apple notarization is configured.

## Design requirements from the user

Flat rounded design inspired by iOS 18, neutral surfaces and configurable accent. Read `DESIGN.md` and `PRODUCT.md` before UI changes. Use rounded sheets and controls, but compact **6px menu highlights**, not oversized pills in the menu bar. Search needs a magnifier, readable text, generous internal spacing and a separate wide row. Library section headings are 16px; result count 14px. Light-mode waves must remain visible.

Pointer press feedback is 160ms; checkmark/radio strokes take **320ms linear**, explicitly slowed at the user's request. Transitions reverse from the current presentation value. Keyboard actions remain immediate. Reduced motion suppresses spatial motion; background pointer response must never consume editor input. Do not animate CAD geometry or reading positions decoratively.

The two background references are adapted native implementations, not identical ports of the original GPU shaders. Preserve `docs/REACT_BITS_LICENSE.md` and distribution notice.

## Data and compatibility

Old `EdgeRef` JSON has no `sweep` field and continues to mean a full circle. New open arcs store an optional angle in degrees and render/select only that sweep. Old application binaries do not understand bounded arcs; use the updated app to edit those projects.

Commit pending sketches before save/close, protect all dirty tabs on quit, retain changes if saving fails, and do not autosave over another project's data. Existing lifecycle tests cover these cases.

## Build and checks

```sh
cargo test --workspace --offline
cargo build --release --offline
bash packaging/macos/bundle.sh target/release/genix
codesign --verify --deep --strict dist/Genix.app
unzip -tq dist/Genix-macOS.zip
```

The first dependency build needs CMake and dependency downloads. Use `--offline` only after dependencies exist. `iconutil` may require execution outside the filesystem sandbox on the development Mac. Local builds are Apple Silicon; the GitHub workflow targets additional platforms. Do not claim an unbuilt platform was tested.

Use `GENIX_SETTINGS_PATH=/absolute/temporary/config.json` for isolated QA. Never change the user's real profile or overwrite their models to test. Finder opening must be checked through LaunchServices on both cold and warm launches; a CLI path alone is insufficient. Record actual proof and limits in `docs/VALIDATION.md`; historical screenshots may precede later visual fixes.

## Current verification

Workspace tests cover partial-arc selection/fillet tools, file preservation, tab closure, search/filtering, checkbox pointer/keyboard input, animation timing/reversal, actual background pointer-event rendering and hole center/axis snapping. One existing heavy core stress test is ignored by default. Read `docs/VALIDATION.md` for the latest counts and native QA status.
