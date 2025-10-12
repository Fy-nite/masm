# MASM

Welcome to the official repository for MASM (Micro-Assembly), A General Purpose Assembly Language Interpreter with Native Interface (MNI) support.

MASM is designed to be a full replacement for other Micro-Assembly Interpreters like JMASM, CMASM, and others, while providing a more modern and extensible architecture. It supports a good chunk of the MicroV2 instruction set and allows for easy integration with native code through its Module Native Interface (MNI).

## Features
- **Cross-Platform**: Runs on Windows, Linux, and macOS.
- **x86-64 Architecture**: Supports a wide range of x86-64 instructions.
- **Module Native Interface (MNI)**: Call native code in host languages like Java, C#, C, C++, Rust, Go, etc.
- **Extensible**: Easily add new modules and functions.
- **Threading Support**: Create and manage threads within MASM programs.
- **Memory Management**: Allocate, reallocate, and free memory dynamically.
- **Standard I/O**: Read from stdin and write to stdout/stderr.
- **Comprehensive Documentation**: Detailed guides and API references.
- **Example Modules**: Pre-built modules for common tasks.
- **Test Suite**: Ensure reliability with a suite of tests.
- **Open Source**: Free to use and modify under the AGPL License.

## Getting Started
1. **Clone the Repository**:
   ```bash
   git clone https://github.com/charlie-sans/masm.git
   cd masm
    ```
2. **Build the Project**:
    ```bash
    cargo build --release
    ```
3. **Run the Interpreter**:
    ```bash
    ./target/release/masm path/to/your/file.masm -o <output>.masi
    ./target/release/masm path/to/your/file.masi
    ```
4. **Explore Example Modules**: Check out the `masm/modules` directory for example modules like `GUI.lua`.
5. **Write Your Own MASM Programs**: Use the provided examples and documentation to create your own MASM programs.
6. **Run Tests**: Ensure everything is working by running the test suite.
    ```bash
    cargo test
    ```
## Documentation
- [User Guide](masm/docs/user_guide.md)
- [API Reference](masm/docs/api_reference.md)
- [Module Development](masm/docs/module_development.md)
- [Assembly Language Reference](masm/docs/MicroV2.md)