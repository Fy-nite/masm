# Module Native Interface (MNI) — Codebase Guide

This document describes how the Module Native Interface (MNI) is implemented and used in this repository. It explains the assembly syntax, binary encoding, runtime behavior, the Python module contract used by the interpreter loader, examples, and where to find the relevant code.

## Overview

MNI provides a unified way for Micro-Assembly programs to call native functions provided by the host/runtime. In this codebase the interpreter implements a small MNI registry and a Python-based loader for external modules. The implementation pieces live in:

- `Sources/Interpreter.swift` — MNI runtime, registry, loader and invocation.
- `Sources/Instructions.swift` — assembler parsing/generation for `MNI` instructions and string interning for `MNI module.func ...` syntax.
- `Sources/Disassembler.swift` — disassembles `MNI` opcodes when printing MASI files.
- `modules/tool.py` — example Python MNI module (class-based) included as a sample.
- `Native-interface.md` and `MicroV2.md` — higher-level design notes and examples.

## Assembly-level usage

You can write MNI calls in two forms in MASM source:

1) Pointer-style (explicit data pointers):

   MNI $<module-data-label> $<function-data-label> [args...]

   Example:

   MNI $__mni_str_0 $__mni_str_1 $R1

   In this form each of the first two operands are data pointers (e.g. `$label`), pointing at C-style null-terminated strings in the data section containing the module name and function name.

2) Short form (convenience):

   MNI ModuleName.FuncName [args...]

   When the assembler parses `MNI Module.Func ...` it automatically interns the module and function names as null-terminated data labels and rewrites the instruction as an `MNI` with `$<data-label>` operands. See `Sources/Instructions.swift` for the interning logic.

Arguments are space-separated tokens after the module/function. The assembler interns string arguments too (when using the short form) and emits their data labels into the data section.

Example:

   ; convenience form — assembler creates data labels for module/function/args
   MNI tool.join "a" "b"

Which becomes at assemble time something like:

   MNI $__mni_str_0 $__mni_str_1 $__mni_str_2 $__mni_str_3

## Binary encoding (MASI)

The `MNI` opcode byte is 0x60. Encoding in the code stream (as implemented by the assembler in `Instructions.CompileInstructions`) is:

- opcode (1 byte) = 0x60
- module operand: (mode u8) + (value u64)
- function operand: (mode u8) + (value u64)
- argc: u16
- for each arg:
  - arg operand: (mode u8) + (value u64)

Operand modes follow the project's generic operand encoding used across instructions:

- mode 0: immediate — the 8-byte u64 value is used as an integer immediate.
- mode 1: register — the 8-byte value is interpreted as a register id (u16 used) and the runtime reads register value when evaluating operand.
- mode 2: label/address in code (label -> code offset)
- mode 3: memory absolute address — the 8-byte value is treated as a byte offset into the data/memory section (commonly used for `$label`).
- mode 4: memory via register id — value contains register id; the runtime reads the register to get the address and loads/stores from memory at that address.

Note: The assembler forces argument labels to be encoded as memory absolute addresses (mode 3) when emitting the `MNI` argument list in the MASI file.

## Interpreter behavior (runtime semantics)

Runtime MNI behavior is implemented in `Sources/Interpreter.swift`. Important points:

- There is a `ModuleRegistry` (Interpreter.ModuleRegistry) mapping module name -> function name -> `MNIFunc`.
- `MNIFunc` is a Swift type alias: `(inout Interpreter.MNICtx) -> Void`.
- `MNICtx` contains:
  - `state: Interpreter.State` — the interpreter snapshot (registers, memory, flags, stack, rip).
  - `args: [String]` — the stringified argument list (see argument conversion rules below).
  - helper `writeString(_:)` to write to stderr.
- When the interpreter executes an `MNI` opcode it:
  1. Reads the module operand and function operand (both are encoded as pointers to data): it uses the module and function operand values as addresses and reads C-strings from the interpreter's `state.memory`.
  2. Reads argc and builds `argv: [String]` by decoding each arg operand according to its mode (details below).
  3. Looks up the registered native function via `registry.lookup(module, name)`. If found, it creates `var ctx = MNICtx(state: state, args: argv)` and calls the function `fn(&ctx)`. Afterwards it replaces `state = ctx.state` to pick up any memory/register changes the MNI function made.
  4. If not found, a short error message is written to stderr: "MNI: function not found".

Argument decoding (how argv is constructed for the callee):

- mode 0 (immediate): the u64 value is converted to a decimal string and appended to `argv`.
- mode 1 (register): the register id is converted to its textual name using `RegisterMap.name(for:)` (fallback `REG<id>`) and that name string is appended to `argv`.
- mode 3 (memory absolute): the interpreter attempts to read a null-terminated C-string at that address from `state.memory`; if successful the string is appended; otherwise a `$0x...` hex string of the numeric value is appended.
- mode 4 (memory via register id): resolves the register id to a register name and appends `"$<REGNAME>"` (the assembler emits `$REG` style for these).

Note: The interpreter currently does not pass raw binary buffers as function arguments — arguments are surfaced as strings (or register / pointer names), and the MNI function can read memory via the address values by reading registers or memory from the `MNICtx.state` directly (if it is implemented in Swift), or, for Python modules, by returning a `store` dict which the interpreter will use to write into memory.

## Python MNI loader contract (modules/*.py)

The interpreter includes a PythonKit-based loader (`Interpreter.loadModules(from:)`) that scans `modules/*.py` and registers functions into the `ModuleRegistry`. Two supported contracts exist:

1) Class-based (preferred):

   - Define a class with a class attribute `MNI_MODULE = "name"`.
   - Methods of the class can be exported by decorating them with `@mni_export()` (or `@mni_export("alias")`). The example `modules/tool.py` shows this pattern.
   - Exported methods are called with signature: `(args: list[str], regs: dict[str,int])`.

2) Dict-based (fallback):

   - The module exposes `MNI_MODULE = "name"` and a `MNI_FUNCTIONS` dictionary mapping function name -> callable.
   - Each callable is called as above: `(args, regs)`.

Return value contract for Python MNI functions:

- The function can return `None`, a `str`, or a `dict`.
- If a `str` is returned, it is printed to stdout.
- If a `dict` is returned it may include the following keys:
  - `out`: string to print to stdout.
  - `regs`: a dict mapping register names (e.g. "RAX") to integer values — the interpreter will update `ctx.state.regs` for those registers.
  - `store`: a dict describing a memory store operation. `store` may contain:
    - `addr`: absolute address (int) to write at, or
    - `reg`: register name whose value is used as the destination address.
    - payload: either `string` (utf-8 text) or `bytes` (a list of integers 0..255) to write. When `string` is provided the interpreter appends a NUL terminator and writes bytes into `state.memory` at the target address.

The loader registers each exported function under `registry.register(module: String, name: String, fn: MNIFunc)` where `MNIFunc` wraps the Python callable and translates argv/regs/result to/from interpreter state. See `Sources/Interpreter.swift` for exact translation code.

Example `modules/tool.py` (included):

```python
class Tool:
    MNI_MODULE = "tool"

    @mni_export()
    def join(self, args, regs):
        return ", ".join(args)

    @mni_export("set_rax")
    def set_rax(self, args, regs):
        # returns a dict to update registers
        return {"out": "RAX <- ...", "regs": {"RAX": 123}}
```

## Example assembly

1) Short form (convenience):

   ; assembler will create data labels for the strings
   MNI tool.join "hello" "world"

2) Pointer form (explicit):

   DB __mni_str_0 "tool\0"
   DB __mni_str_1 "join\0"
   ; later in code
   MNI $__mni_str_0 $__mni_str_1 $R1

3) Debug helper (registered at runtime in Swift interpreter):

   ; the interpreter registers `debug.echo` which reads RDI as pointer to a C-string
   MNI Debug.echo $RDI

## Disassembly

The disassembler prints MNI as:

   MNI <fmtOp(module)> <fmtOp(function)> [args...]

Where `fmtOp` reconstructs a readable operand from the encoded mode/value (register name, `$label` for data, `#label` for code labels, immediates, etc.). See `Sources/Disassembler.swift`.

## Adding a new Python MNI module

1. Create `modules/<name>.py` inside the repository or in your working directory's `modules/` directory.
2. Either use the class-based or dict-based contract. For the class-based approach, provide `MNI_MODULE` and decorate exported methods with `@mni_export()`.
3. Methods should accept `(args, regs)` where `args` is a list of strings (the assembled argument rendering) and `regs` is a dict mapping register names to integers.
4. Return `None`, `str`, or `dict` with optional `out`, `regs`, and `store` fields (see above).
5. Run the interpreter; it automatically loads modules from `./modules` at startup.

## Implementation notes & limitations

- MNI arguments are surfaced to Python as strings (or register name tokens). If you need raw buffers, use the `store` convention or use register addresses/pointers and read/write `ctx.state.memory` from a native (Swift) MNIFunc implementation.
- The Python loader depends on PythonKit and will only run when PythonKit is available (the code uses `#if canImport(PythonKit)` around loader code).
- Security: loading arbitrary Python from `modules/` executes code in-process; treat modules as trusted.
- The interpreter prints a short message when a function is missing; there is no exception thrown into MASM code.

## Source references

- Assembler parsing & encoding: `Sources/Instructions.swift` (look for `case .mni`, string interning, and encoding of `mni` in CompileInstructions).
- Disassembly: `Sources/Disassembler.swift` (case 0x60).
- Runtime/loader/registry: `Sources/Interpreter.swift` (search for `MNICtx`, `ModuleRegistry`, and `loadModules`).
- Example Python MNI module: `modules/tool.py`.
- Higher-level design notes: `Native-interface.md` and `MicroV2.md` contain user-facing examples and suggested MNI functions.

## Quick checklist for contributors

- [ ] If you add a new MNI function implemented in Python, add it to `modules/` and follow the class/dict contract.
- [ ] If you change argument encoding or operand modes, update `Instructions.swift`, `Disassembler.swift`, and `Interpreter.swift` together.
- [ ] Add unit tests or small MASI examples demonstrating any new functionality.

---

If you'd like, I can also add a small runnable example MASM file + assemble/run steps that calls a Python MNI function from `modules/tool.py` and show the interpreter output.
