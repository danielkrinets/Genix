# Genix for scripts and AI agents

Genix has a headless command line, so a script or an AI agent can build a model, check it and export it without the window.
Everything below runs the same engine as the app. Results are JSON on stdout; errors go to stderr with a non-zero exit code.

| Command | What it does |
|---|---|
| `genix --new model.genix` | creates an empty document |
| `genix --features` | prints the feature types and ready-to-use JSON examples (take the exact encoding from here) |
| `genix --add model.genix '<feature json>'` | appends a feature (`-` reads the JSON from stdin). If the feature does not build, the file is **not** changed and the error is printed |
| `genix --eval model.genix` | evaluates the whole document: per-feature errors, bodies with id, name, volume (mm³), bounding box, overlaps. Exit code 3 if any feature has an error |
| `genix --export model.genix out.stl` | exports all bodies; `.stl`, `.obj`, `.3mf`, `.step`, `.dxf` (STEP and DXF carry the exact triangles: facets, no curved surfaces) |
| `genix --render-frames model.genix out_dir` | PNG frames of every build step and a turntable |

## Typical agent loop

1. `genix --new part.genix`
2. `genix --features` once, copy the JSON shape of the feature you need.
3. `genix --add part.genix '{"Primitive":{...}}'` — read `bodies[].id` from the reply; later features refer to bodies by that id.
4. After every step look at `ok`, `features[].error`, `bodies[].volume_mm3`, `bodies[].bounds`, `interferences`. Fix and retry; a failed `--add` leaves the file untouched.
5. `genix --export part.genix part.stl`.

Units are millimetres and degrees. Any number may also be a string expression that uses named parameters (`"w*2"`), see `params` in the file.
The file is plain JSON (`format: "genix"`); it can be edited directly, and `genix --eval` tells whether the result still builds.

Limits: the local app has no account or server; the engine is mesh-based (see `docs/HANDOFF.md` for what is not modeled yet).
