# MASM Diagnostics

This document lists error and warning codes the MASM toolchain can emit across the assembler, linker, and VM runtime. Messages follow a Rust-like style for clarity, with codes, short descriptions, and helpful notes.

## Conventions

- Errors: `error[CODE]: message`
- Warnings: `warning: message`
- Location hints may include `--> pc: 0x...` or file/line when available.

## Assembler Errors

- EASM001: Unrecognized instruction or operand count
  - Example: `error[EASM001]: unrecognized instruction or operand count\n  --> <input>: ?\n   = help: got: 'FMOO R1, R2'`
  - Fix: Check mnemonic and operand arity.
- EASM002: Duplicate label
  - Example: `error[EASM002]: duplicate code label 'main'`
  - Also applies to data labels.
  - Fix: Rename or remove duplicates.
- EASM004: Include depth exceeds limit
  - Example: `error[EASM004]: include depth exceeds limit (depth=33)`
  - Fix: Simplify includes or break cycles.

## Linker Errors

- ELD001: Duplicate export symbol
  - Example: `error[ELD001]: duplicate export symbol 'code main'`
  - Fix: Export only once or rename.
- ELD002: Unresolved import
  - Example: `error[ELD002]: unresolved import: code foo\n   = help: ensure the providing object exports this symbol or link it before users`
  - Fix: Link the provider first or add export.

## VM Runtime Errors

- EVM001: Program counter moved past end of code
  - Example: `error[EVM001]: program counter moved past end of code\n  --> pc: 0x51\n   = note: attempted to execute at 0xDEADBEEF, code size 82`
  - Cause: invalid jump/ret target usually due to stack misuse.
- EVM002: Return address outside code section
  - Example: `error[EVM002]: return address outside code section\n  --> pc: 0x40\n   = note: return target 0x1000000, code size 256\n   = help: did you POP the return address by accident? (stack imbalance)`
  - Fix: Don’t pop the return address; use registers for arguments or a proper prologue/epilogue.
- EVM003: Invalid call target
  - Example: `error[EVM003]: invalid call target\n  --> pc: 0x40\n   = note: call target 0x1000000, code size 256`
  - Cause: CALL to address outside code region (often corrupted or incorrect pointer).
  - Fix: Ensure targets are code labels or valid addresses within code.
- EVM004: Invalid jump target
  - Example: `error[EVM004]: invalid jump target\n  --> pc: 0x44\n   = note: jump target 0xDEAD, code size 1024`
  - Cause: conditional/unconditional jump target outside code.
  - Fix: Initialize labels/branch targets correctly.

## VM Runtime Warnings

- OOB read: `warning: out-of-bounds memory read at 0xADDR (mem size N)\n  --> pc: 0xPC`
- Memory growth on write: `warning: write extended memory from OLD to NEW\n  --> pc: 0xPC\n   = note: store at 0xADDR (size 8)`
- Memory growth on IN: `warning: IN extended memory from OLD to NEW\n  --> pc: 0xPC`
- OUT invalid string address: `warning: OUT read invalid string address $0xADDR\n  --> pc: 0xPC`
- COUT OOB: `warning: COUT read out-of-bounds at 0xADDR\n  --> pc: 0xPC`
- POP on empty stack: `warning: POP on empty stack\n  --> pc: 0xPC`
- FP NaN/Inf result: `warning: FP result is NaN|infinite\n  --> pc: 0xPC`
- Unknown OUT port: `warning: unknown OUT port P (defaulting to stdout)\n  --> pc: 0xPC`
- Unknown COUT port: `warning: unknown COUT port P (defaulting to stdout)\n  --> pc: 0xPC`

## Roadmap

Future candidates:

- Assembler: invalid addressing/operand type codes; better source spans.
- Linker: conflicting entry points; duplicate imports with different kinds.
- VM: division-by-zero integer ops; syscall argument validations.

Contributions welcome—please keep codes stable once shipped and add examples and fixes for each diagnostic.
