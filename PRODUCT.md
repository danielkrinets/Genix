# Genix

## Product and workflow
A native parametric CAD application. The user's requested flow is registration, then a personal model library with previews and convenient access to settings, profile, updates and a server subscription, then the existing editor.

## Platform and stack
Existing Rust desktop application using egui/wgpu and Manifold. Preserve the native editor and its current visual system. The repository also contains iOS and other desktop packaging; local validation is on macOS Apple Silicon.

## Confirmed scope
Registration and server subscription are explicitly mocked for now. Do not charge money, send credentials, or claim server storage is active. Keep mock status visible. Models and account preferences are local. Show real geometry previews. Existing modeling, undo, save, reopen and STL export must keep working.

## Updates
Show the installed version and provide access to official GitHub builds. Never invent the availability of a newer version.

## Undecided
Server pricing, storage capacity, production authentication, payment provider, backend and synchronization are not specified. Do not invent commercial commitments.

## Confirmed visual direction
The user explicitly requests flat rounded design inspired by iOS 18 across all surfaces, including docks, loading indicators, sliders and animations. One configurable accent must propagate consistently. Preserve modeling behavior, readable contrast and immediate keyboard shortcuts; support reduced motion.
