@AGENTS.md

AGENTS.md (imported above) holds the hard rules and the skill router and stays the source of
truth for them; `cargo xtask skills` keeps it under 5000 characters, so detail does not go
there. This file adds the codebase map and the working notes for this checkout. Do not
restate a rule from AGENTS.md here: link the skill instead.

## This checkout

- Upstream is `origin` (tangency-labs/solidcraft, branch `main`). It moves fast: hundreds of
  commits a week, with `v*` release-candidate and nightly tags.
- This is the CAM fork: remote `cam` (HansFel/SolidCraftCam), branch `cam-linuxcnc`. CAM
  work is committed here and pushed to `cam`, never to `origin`. Upstream is merged in
  (`Merge origin/main ...`), not rebased.
- Commit messages on the branch follow `cam: <what changed, in plain words>`, in English.
- Other agents may be working in the same tree. Uncommitted changes that are not yours stay
  untouched: stage your own paths by name (`git add <files>`), never `-A`, and do not stash,
  reset or amend. `plan/` is git-ignored; `plan/COORDINATION.md` holds the local claims.
- A second cargo run waits on the `target/` lock. Use your own `CARGO_TARGET_DIR` when
  another agent is building, and do not overwrite a `target/release/solidcraft.exe` that is
  running.

## Commands

```sh
cargo xtask ci                      # the gate before a commit: fmt, asset/list/layer checks, clippy -D warnings, tests, wasm
cargo xtask test -- <filter>        # all test binaries with the fast `ci` profile; the filter goes to each binary
cargo test -p solidcraft-cam        # one crate
cargo clippy -p solidcraft-cam --all-targets -- -D warnings
cargo xtask lists --fix             # sort the message catalogs
cargo xtask wasm                    # check every library crate and the web app for wasm32
cargo run --release -p solidcraft -- --sample              # desktop app with a sample part
cargo run --release -p solidcraft-cli -- commands          # the command registry
cargo run --release -p solidcraft-cli -- run examples/bracket.json --out bracket.step
```

`cargo xtask` is an alias for `cargo run --package xtask --` (`.cargo/config.toml`); run it
with no arguments for the full list of checks. More in `docs/building.md`.

On this Windows machine: cargo lives in `%USERPROFILE%\.cargo\bin` and is not on the Git Bash
PATH. A full build takes 40 minutes or more, and heavy test runs can exhaust the page file
(os error 1455): limit them with `-j 2` or `SOLIDCRAFT_TEST_JOBS=2`. The repository is LF
only; a script that rewrites files must not turn them into CRLF.

Formatting is `max_width = 150` with `use_small_heuristics = "Max"` (`rustfmt.toml`).
`unsafe_code` is forbidden workspace-wide; unwrap, expect, panic and indexing are allowed in
tests only (`clippy.toml`).

## Workspace map

Rust 2024, one Cargo workspace: `crates/*`, `apps/*`, `xtask`. `vendor/` holds patched
copies of egui-wgpu and winit and is excluded from linting. A lower layer never depends on a
higher one (`cargo xtask layers`, table in `docs/architecture.md`).

| Layer | Crate | What it holds |
|---|---|---|
| L0 | `crates/geom` | vectors, planes, frames, profiles, meshes and their measures |
| L1 | `crates/sketch` | sketch entities, constraints, the solver, profile detection |
| L1 | `crates/kernel` | the B-rep kernel boundary behind our own API: solids, booleans, blends, measures |
| L1 | `crates/smlib` | safe wrapper over solidcore, the external kernel pinned by rev in `Cargo.toml` |
| L1 | `crates/step` | STEP Part 21/42 as plain data |
| L1 | `crates/render` | camera and view math, headless scene descriptions, product render jobs |
| L1 | `crates/cam` | tools, toolpaths, 2.5D milling, plasma, G-code and posts |
| L1 | `crates/dynamics` | multibody dynamics |
| L2 | `crates/doc` | parameters, expressions, the feature timeline and its incremental evaluation, the design format |
| L3 | `crates/io` | design files, STL/OBJ/STEP/IGES/3MF/glTF, FreeCAD and Rhino import |
| L4 | `crates/engine` | the session, the command registry, undo/redo, selection, command schemas |
| L5 | `crates/gpu` | viewport render pipelines and compute-shader tessellation |
| L5 | `crates/ui-egui` | the egui + wgpu front end (the largest crate with `engine`) |
| L5 | `crates/mcp` | MCP server over stdio, headless or bridged to the running app |
| apps | `apps/solidcraft`, `apps/solidcraft-cli`, `apps/solidcraft-web` | desktop app, headless CLI, browser build (Trunk) |

Helpers: `crates/derive` (proc macros), `crates/profile` (timed spans), `crates/testkit`
(dev-dependency only), `crates/space-mouse` and `crates/threedx-client` (3D mouse input).

Other top-level directories:

- `xtask/`: every repository check and generator (`xtask/src/main.rs` lists them).
- `tests/`: cross-crate corpora: `kernel-cases/` (the rig and the ladder), `oracle/`
  (reference recipes), `action_matrix/`, STEP conformance and regressions,
  `KNOWN_FAILING_TESTS`.
- `assets/`: icons, fonts, message catalogs (`assets/locales/`); read `assets/AGENTS.md`
  before icon work.
- `docs/`: design notes per topic; `docs/book/` is the mdBook (user guide in `guide/`,
  developer chapters in `dev/`: commands, schemas, testing, kernel, naming, UI kit).
- `skills/`: the agent skills AGENTS.md routes to.
- `examples/`: command scripts and sample designs the CLI can run.
- `packaging/`, `scripts/`, `tools/`: installers and containers, CI and repository scripts,
  reference-value tools (oracles, kernel ladder).

## How a change flows

Every user-visible action is a command reached through `Session::execute(id, params)`: the
ribbon, dialogs, scripts, the CLI, the control channel and MCP all call the same registry.

1. Geometry and algorithms go in the lowest crate that can hold them (`geom`, `sketch`,
   `kernel`, `cam`).
2. Saved state goes in `crates/doc`; a format change is one new file in
   `crates/doc/src/format/pending/`.
3. The command is a `CommandSpec` in `crates/engine/src/cmd/<area>.rs`, registered in
   `crates/engine/src/cmd/mod.rs`, with its contract and reply type in
   `crates/engine/src/schema/`.
4. The UI in `crates/ui-egui` stays thin: a dialog builds the parameters and runs the
   command. Labels and help go in `assets/locales/pending/<slug>.json` as `cmd.<id>` and
   `help.<id>`.
5. Tests sit beside the code or in the area's `*_tests.rs`, measured against values derived
   by hand; UI scenarios are JSON in `crates/ui-egui/tests/scenarios/`.

## CAM (this fork's work)

Generated G-code moves a machine: every program is read back and compared with the planned
toolpaths before export, and the UI and docs say what is and is not checked. Behaviour comes
from general CAM practice and public controller manuals only.

| Where | What |
|---|---|
| `crates/cam/src/` | `ops.rs` operations, `path.rs` and `offset.rs` toolpath geometry, `tool.rs`, `machine.rs`, `cutting.rs` cutting data, `gcode.rs` and `post.rs` posts, `klartext.rs` Heidenhain plain-text output, `comp.rs` cutter compensation, `lead.rs`, `tabs.rs`, `helix.rs`, `slot.rs`, `plasma.rs`, `readback.rs` program check, `sim.rs` material removal, `time.rs`, `check.rs`; all tests in `tests.rs` |
| `crates/doc/src/cam.rs` | the saved CAM job; format file `crates/doc/src/format/pending/cam-milling.rs` |
| `crates/engine/src/cmd/cam.rs`, `cam_job.rs`, `cam_sheet.rs`, `cam_collide.rs` | the `cam.*` commands (`cam.setup`, `cam.tool.define`, `cam.op.contour`, `cam.op.pocket`, `cam.op.drill`, `cam.post`, `cam.export`, `cam.simulate`, ...) |
| `crates/engine/src/schema/cam.rs` | their input and output schemas |
| `crates/ui-egui/src/cam_panel.rs`, `dialogs_cam.rs`, `dialogs_cam_setup.rs`, `cam_manual.rs` | job list and tool library panel, operation and setup forms, in-app manual; tests in `dialogs_cam_tests.rs` |
| `assets/locales/pending/cam-linuxcnc.json` | every CAM UI string, kept sorted |
| `attribution/pending/cam-linuxcnc.md`, `assets/icons/ui/design/svg/model/cam_*.svg` | CAM icons and their attribution rows |
| `docs/cam-linuxcnc.md`, `docs/book/src/guide/cam.md` | user documentation; update it with the feature |
| `CAM_Roadmap.md` | status (Done, Partial, Planned) and plan; update it when an item lands |
| `examples/CAM/` | sample CAM designs and tool libraries |

`joints_cam*` and `camera*` files are the cam joint of assemblies and the view camera, not
machining.

Build and delivery: a push to `cam-linuxcnc` on the fork runs
`.github/workflows/cam-build.yml`, which builds the Linux app (tar.gz, rpm) and the web site
and publishes them as the rolling pre-release `cam-latest`. `scripts/cam-update.sh` installs
that release on a machine, so nothing is compiled there.
