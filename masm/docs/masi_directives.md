# MASM Import/Export Directives and Linking

## Import/Export Directives

MASM source files support preprocessor directives for symbol import and export:

- `#export <label>`: Marks a code label (`#foo`) or data label (`$bar`) for export. These will be included in the MASI export table.
- `#import [code|data] <name>`: Declares an external symbol to be imported. Use `code` for functions/labels, `data` for variables.

**Examples:**
```
#export #main        ; Export code label 'main'
#export $buffer      ; Export data label 'buffer'
#import code extfunc ; Import external function 'extfunc'
#import data extvar  ; Import external variable 'extvar'
```

## Export/Import Table Format
- Export Table: count (u16), then entries: kind (u8, 0=code, 1=data), nameLen (u16), name bytes, offset (u64)
- Import Table: count (u16), then entries: kind (u8), nameLen (u16), name bytes, refCount (u16), then for each reference: section (u8), offset (u64)

## Linking MASI Files

A new linker is available via the CLI:
```
masm link <input1.masi> <input2.masi> ... -o <output.masi>
```
This command merges multiple MASI files, resolves imports using exports, and produces a single linked MASI file. All code and data sections are concatenated, and symbol addresses are re-based. Imports are patched to the correct addresses if matching exports are found.

**Typical workflow:**
1. Assemble modules with `#export` and `#import` as needed.
2. Link them together:
   ```
   masm link a.masi b.masi -o final.masi
   ```
3. Run or disassemble the linked output as usual.

## Notes
- Numeric memory addresses like `$1000` or `$0x3000` are always treated as direct addresses, not imports.
- Unresolved imports during linking will result in an error.
- The disassembler (`masm dis <file.masi>`) now shows import/export tables for inspection.
