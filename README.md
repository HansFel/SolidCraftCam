<p align="center">
  <img src="https://raw.githubusercontent.com/tangency-labs/solidcraft/main/assets/icons/app/linux/exports/hicolor/256x256/apps/io.tangencylabs.solidcraft.png" width="96" height="96" alt="SolidCraft CAM" />
</p>

<h1 align="center">SolidCraft CAM for LinuxCNC</h1>

<p align="center">
  <strong>2.5D CAM toolpath generator for SolidCraft parametric 3D CAD</strong><br />
  Generate professional toolpaths for LinuxCNC from your SolidCraft designs.
  Fork of SolidCraft with integrated CAM functionality for CNC machining automation.
</p>

<div align="center">

[![License](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue?style=flat-square)](LICENSE)
[![Repository](https://img.shields.io/badge/GitHub-HansFel%2FSolidCraftCam-black?style=flat-square&logo=github)](https://github.com/HansFel/SolidCraftCam)
[![Discord](https://img.shields.io/badge/Discord-SolidCraft-5865F2?logo=discord&logoColor=white&style=flat-square)](https://discord.gg/FtU4NQNVDH)

</div>

## About

**SolidCraft CAM** extends [SolidCraft](https://github.com/tangency-labs/solidcraft) – an open-source parametric 3D CAD application built in pure Rust – with **2.5D CAM (Computer-Aided Manufacturing)** capabilities for [LinuxCNC](http://linuxcnc.org/).

This project enables you to:
- Design parts parametrically in SolidCraft
- Generate 2.5D toolpaths directly from your designs
- Export G-code compatible with LinuxCNC and other CNC controllers
- Integrate CAM workflows into your manufacturing pipeline

## Features

- **2.5D Toolpath Generation** – Create toolpaths for pocket, facing, and contouring operations
- **LinuxCNC Integration** – Export G-code ready for LinuxCNC controllers
- **Parametric CAM** – Update toolpaths automatically when design parameters change
- **SolidCraft Foundation** – All SolidCraft features: sketches, constraints, features, assemblies
- **Open Source** – MIT or Apache-2.0, fully transparent development

## Getting Started

### Prerequisites

- Rust toolchain (latest stable)
- SolidCraft dependencies (see [SolidCraft docs](https://docs.solidcraft.io/))
- LinuxCNC (for testing G-code output, optional for CAM generation)

### Installation

```bash
# Clone this repository
git clone https://github.com/HansFel/SolidCraftCam.git
cd SolidCraftCam

# Build the CAM branch
git checkout cam-linuxcnc
cargo build --release

# Run the desktop app
cargo run --release -p solidcraft -- --sample
```

### Basic CAM Workflow

1. **Design your part** in SolidCraft (sketches, features, parameters)
2. **Switch to CAM mode** (cam-linuxcnc branch features)
3. **Define toolpath operations** – select faces, set tool diameter, depth of cut
4. **Generate G-code** – export ready-to-run toolpaths
5. **Run on LinuxCNC** – load the G-code into your CNC controller

### Example

```bash
# Generate G-code from a design
cargo run --release -p solidcraft-cli -- cam examples/bracket.sc \
  --tool-diameter 3.175 \
  --output bracket.ngc
```

## Architecture

SolidCraft CAM is built on top of SolidCraft's modular architecture:

- **`crates/solidcraft-kernel`** – Core CAD geometry and modeling
- **`crates/solidcraft-cam`** – CAM-specific toolpath generation (this project's main contribution)
- **`crates/solidcraft-cli`** – Command-line interface for batch CAM operations
- **`apps/solidcraft`** – Desktop application with integrated CAM UI

## Project Status

🚀 **In Active Development** – The cam-linuxcnc branch is currently under development with core 2.5D CAM functionality. More advanced features (3D toolpaths, multi-axis) are planned.

See [CAM_Roadmap.md](CAM_Roadmap.md) for detailed feature roadmap and development plans.

## Plugins (experimental desktop runtime)

Plugins add separately distributed behaviour without compiling that behaviour into SolidCraft.
The initial runtime loads **WebAssembly components**, explicitly requested by the user. Merely
installing a folder, enabling it in Preferences, or opening a design never executes plugin code.
The interface is experimental: WIT package `craft:plugin@0.1.0`, capability `commands@0.1`. While
it is 0.x a minor version may break the one before it; the host refuses a plugin that needs a
capability it does not have, in plain words. Review a plugin again after an update. The desktop app and headless CLI use the same commands. Browser
execution, automatic downloads and updates, custom panels, and plugin-defined parametric feature
types are future work.

### Install and inspect

A package is a folder named after its id, containing `plugin.json` and a `.wasm` component.
Preferences > Plugins lists packages under the per-user data folder: `%LOCALAPPDATA%/SolidCraft/plugins`
on Windows, `~/Library/Application Support/SolidCraft/plugins` on macOS, and
`${XDG_DATA_HOME:-~/.local/share}/solidcraft/plugins` on Linux. **Show in folder** opens this location.
You can also inspect and explicitly run a package in another folder. Nothing is downloaded by the host.

A minimal manifest looks like this. Plugin packages and examples are maintained outside this
repository, as specified in [the plugin RFC](docs/rfcs/0002-plugins.md).

```json
{
  "id": "example-box",
  "name": "Example box",
  "version": "0.1.0",
  "description": "Create an ordinary editable box from dimensions.",
  "capabilities": ["commands@0.1"],
  "runtime": {
    "module": "plugin.wasm",
    "entries": ["make"],
    "commands": ["solid.box"],
    "limits": {"fuel": 10000000, "memory_mib": 64, "deadline_ms": 5000}
  }
}
```

Run `plugin.inspect` with `{"path":"/path/to/example-box"}` through the CLI's JSON scripts, MCP
`execute`, or the desktop control channel's `engine.execute`. It reads the manifest and component
without executing them, returning the declared commands, limits and an `approval` SHA-256 hash.
Review the author, code and declared rights before using that hash. Inspection does not approve a
package. Existing metadata-only packages remain listable but cannot run until they declare a runtime.

### Run explicitly

After reviewing the inspected package, execute:

```json
{
  "command": "plugin.run",
  "params": {
    "path": "/path/to/example-box",
    "approval": "<exact approval hash returned by plugin.inspect>",
    "entry": "make",
    "params": {"length": 20, "width": 10, "height": 3}
  }
}
```

For example, save that command in a JSON array and use `solidcraft-cli run commands.json`.
For the running desktop app, send it as the `command` and `params` of a standalone
`engine.execute` request (see [control protocol](docs/control-protocol.md)). The native host runs
it on a worker and keeps the window responsive. Stop or a design change discards the pending
result; the host does not automatically rerun external code. Desktop batches and scripts must send
`plugin.run` separately. Headless CLI scripts may include it as a normal command.

The hash binds **both the exact manifest bytes and the component bytes**, so an updated package
requires a fresh inspection and permission review. It is a content identity, not a signature or an
author's reputation. Agent chat asks before executing external plugin code. MCP and the control
channel keep their existing file scopes: allow the package folder explicitly where needed. Naming
a package never grants access to another folder.

### User resource ceilings

The invocation defaults remain **10,000,000 fuel, 64 MiB of guest memory and 5000 ms**.
The default host ceilings remain **100,000,000,000 fuel and 128 MiB**. A package can declare
larger fuel or memory budgets, but execution is refused until you explicitly raise the host
ceilings and approve that exact package content. `plugin.inspect` is inert and reports the
requested limits, current `host_limits`, `within_host_limits` and any `limits_error` even when
a request is too large.

In the native GUI, open **Preferences > Plugins**, enter **Max plugin fuel** and
**Max plugin memory (MiB)**, then choose **Apply** or **OK**. Enter also accepts both values;
**Cancel** reverts changes since the last Apply and **Restore defaults** resets this page.
The settings persist in your user preferences and do not travel with saved designs.

For standalone CLI or headless MCP sessions, create a UTF-8 JSON file, for example
`plugin-limits.json` containing:

```json
{"max_fuel": 200000000000, "max_memory_mib": 512}
```

Pass it explicitly when starting the process:

```text
solidcraft-cli --plugin-limits plugin-limits.json run inspect-plugin.json --results
solidcraft-cli --plugin-limits plugin-limits.json mcp --headless --allow-path /path/to/plugins
```

Missing fields use the original host ceilings. Connected desktop sessions use the GUI's
preferences; CLI settings cannot override them. Positive integer fuel values up to `u64::MAX`
are accepted without rounding. Guest memory is limited to 4096 MiB by the current Wasm32 ABI;
that technical boundary requires a different runtime ABI to change. Neither setting changes
a package's declared budget, executes it, adds permissions, or retries a failed invocation.
Review its manifest and component, obtain the content approval hash, and explicitly invoke it.
A changed manifest budget requires a fresh approval hash. Keep the selected ceilings within your
machine's available resources; they are limits on guest execution, not total SolidCraft memory
or native CAD work. The independent maximum execution deadline stays 30 seconds.

### Permissions, results and saved designs

A plugin can call any command in the MCP catalog whose exact id its manifest lists. Commands that
reach past the open design are grouped: `file` (saving, exporting, importing, inserting from files,
libraries), `doc` (opening or switching designs), `app` (settings, window, diagnostics) and `plugin`
(inspecting or running plugins). Each group is refused unless the manifest requests it in
`runtime.permissions.command_groups` and the user grants it; this version has no grant UI yet.
Filesystem calls require matching manifest permissions and approved folder grants. The clock
requires its manifest permission; otherwise it returns a fixed epoch. No WASI, socket or process
API is linked. A design container reserves `plugins/<plugin-id>/` for a plugin's own data, kept
and saved back untouched when the plugin is missing; plugins cannot read or write it until the
`storage` capability arrives.
Inspection commands can disclose model data, so review them as permissions too. In particular, the optional
`model.mesh` permission exposes the requested body's evaluated vertices and triangles in its owning
component's coordinates (millimetres), with explicit transfer limits. It performs no conversion.

Edits run against an isolated copy of the design. Only a successful call is adopted, as **one Undo
step**. A trap, exhausted fuel, memory limit, invalid result or refused host call discards the
transaction, preserving the previous design and undo/redo history. The host validates produced
native features before accepting them. A plugin that catches a refused host call cannot commit an
earlier partial edit. CPU-heavy guest loops are bounded by fuel and a deadline; the guide explains
the separate cost of compiling a component and executing a granted native command.

The result is ordinary native features: they save, undo, redo and rebuild without the plugin.
Removing the package disables future invocations; it does not remove the features already created.
Changing the plugin's own parameters means another invocation in this first version; there is no
plugin-owned timeline evaluation hook yet. Turn an installed plugin off in Preferences > Plugins
to refuse subsequent desktop calls. Disabling it or changing resource ceilings during a pending
call prevents adoption. CLI
processes have their own session policy and do not read the desktop app's preferences.

See the [runtime and author guide](docs/plugin-runtime.md) for the complete manifest, supported
command boundary, limits, build options, tests and current restrictions.
For step-by-step package checks and expected results, see the [English test guide](docs/plugin-test-guide.md). The existing
[parametric plugin design](docs/plugins.md) remains a separate future phase.

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on:
- Building and testing
- Code style and conventions
- Clean-room contribution rules
- Where help is most needed

**Areas looking for contributions:**
- Advanced toolpath strategies (finishing passes, adaptive feeds)
- Tool library management
- G-code post-processors for other CNC controllers
- Documentation and examples
- Testing and bug reports

## Community

- **Discord:** [Join the SolidCraft Discord](https://discord.gg/FtU4NQNVDH) – Ask questions, share work, discuss features
- **Issues:** [GitHub Issues](https://github.com/HansFel/SolidCraftCam/issues) – Report bugs and request features
- **Discussions:** [GitHub Discussions](https://github.com/tangency-labs/solidcraft/discussions) – General discussions

## Development

### Build from Source

```bash
# Full build and test suite
cargo xtask ci

# Build just the CAM CLI
cargo build --release -p solidcraft-cli

# Run tests
cargo test --all

# Check code style
cargo fmt --check
cargo clippy --all
```

### Understanding the Code

Start with [CLAUDE.md](CLAUDE.md) for AI-assisted development guidance, and see the SolidCraft [architecture documentation](https://github.com/tangency-labs/solidcraft/blob/main/docs/architecture.md).

## Related Projects

- **[SolidCraft](https://github.com/tangency-labs/solidcraft)** – The parametric CAD foundation
- **[SolidCore](https://github.com/bherbruck/solidcore)** – Alternative Rust CAD kernel
- **[LinuxCNC](http://linuxcnc.org/)** – Open-source CNC control software
- **[Machine-neutral CAM Companion](https://github.com/mountainarcwelding/campanion)** – Another SolidCraft CAM project

## License

This project is licensed under **MIT OR Apache-2.0**, at your option.

- All SolidCraft files retain their original licenses
- See [DISCLAIMER.md](DISCLAIMER.md) for additional terms
- Third-party code and assets retain their documented licenses (see [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt))

## Disclaimer

The files in this repository are provided as is, without warranty, liability or indemnity. Use at your own risk, especially when running CNC operations. Test all toolpaths on scrap material first.

## Support

- 🐛 **Found a bug?** Open an [issue](https://github.com/HansFel/SolidCraftCam/issues)
- 💡 **Have an idea?** Start a [discussion](https://github.com/tangency-labs/solidcraft/discussions)
- 🤝 **Want to contribute?** See [CONTRIBUTING.md](CONTRIBUTING.md)
- 💬 **Need help?** Join the [Discord community](https://discord.gg/FtU4NQNVDH)

SolidCraft's brand and interface are licensed separately, under
[LICENSE-BRAND](LICENSE-BRAND): the UI kit, the look (interface icons, glyphs, built-in themes),
the theme's design tokens and palettes, and the SolidCraft name, logo and app icon. Use them freely
as part of SolidCraft. Using them in another program or product needs the maintainers' written
permission first.

SolidCraft is independent. Behaviour is designed from public documentation, general CAD
conventions and user feedback. Reference tests use analytic values and open-source tools.
SolidCraft is not affiliated with, sponsored by or endorsed by Autodesk, Dassault Systèmes, PTC,
Siemens or any other CAD vendor. Fusion, AutoCAD, Inventor, SolidWorks, Onshape, Creo, Blender and
Tinkercad are trademarks of their respective owners, named only to describe compatibility
(see [NOTICE](NOTICE)).
---

**Made with ❤️ for the maker and CNC community**

*Developed under the SolidCraft [clean-room contribution rules](AGENTS.md)*
