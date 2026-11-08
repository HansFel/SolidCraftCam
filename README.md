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

---

**Made with ❤️ for the maker and CNC community**

*Developed under the SolidCraft [clean-room contribution rules](AGENTS.md)*
