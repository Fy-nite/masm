; memory.inc - helpers for memory allocation and pointer helpers using MNI Memory.*
; Usage:
;   %include "memory.inc"
;   ALLOC result_reg size_reg    ; pushes args for MNI Memory.allocate
;   FREE ptr_reg                 ; pushes args for MNI Memory.free

; These macros only push/format arguments. Actual MNI invocation should be
; performed by the caller using the assembler's MNI encoding (e.g. `MNI Memory.allocate`).

MACRO ALLOC result_reg size_reg
    ; convention: push size then destination register (or use direct MNI call)
    PUSH $size_reg
    PUSH $result_reg
ENDMACRO

MACRO FREE ptr_reg
    PUSH $ptr_reg
ENDMACRO

; WRITE_STR_TO_PTR ptr_reg label
; A convenience macro stub: the runtime currently exposes host_read/host_write
; helpers for native modules and a Memory MNI can be implemented for efficient
; bulk writes. Until then, callers can either call a small MNI that copies a
; string into heap memory, or expand a manual copy loop.
;
; Example (manual copy using a helper MNI named Memory.write_bytes could be used):
;   MOV RSI label           ; source address (label)
;   MOV RDI ptr_reg         ; dest address
;   MOV RCX length          ; length
;   MNI Memory.write_bytes  ; (example MNI)
;
MACRO WRITE_STR_TO_PTR ptr_reg label
    ; Default expansion performs a simple move of the label address into R2
    ; leaving the actual byte-copy to the caller or a small MNI helper.
    MOV R2 label
ENDMACRO

; ------------------------------------------------------------------
; MNI feature list (available / recommended)
;
; Built-in (present):
; - Memory.allocate <size> <dest_reg>
;     Allocates `size` bytes at end of flat memory, returns base pointer in <dest_reg> (R1 in some tests).
; - Memory.free <ptr>
;     Free is a no-op in the current flat memory model but present for portability.
;
; Suggested / helpful (not yet implemented in Rust core):
; - Memory.write_bytes <dest_addr> <src_label>
;     Copies the NUL-terminated string at <src_label> into memory at <dest_addr>. Returns bytes written in RAX.
; - Memory.read_bytes <src_addr> <dest_label_or_buf>
;     Reads a number of bytes from memory into a buffer or into a label-defined area.
; - Memory.realloc <ptr> <new_size>
;     Reallocate previously allocated block to new size; returns new pointer or error code.
;
; High-level convenience macros that emit the MNI call directly. Use these when
; your assembler supports the `MNI Module.Function args...` syntax (the runtime
; recognizes the `Memory.allocate` and `Memory.free` MNI functions by default).

MACRO MALLOC size dest_reg
    ; Allocates `size` bytes and places base pointer into `dest_reg`.
    MNI Memory.allocate size dest_reg
ENDMACRO

MACRO FREE ptr
    MNI Memory.free ptr
ENDMACRO

MACRO WRITE_BYTES dest label
    ; Calls a proposed helper MNI that would copy a NUL-terminated string
    ; located at `label` into `dest`. This MNI is not implemented by default
    ; in the Rust runtime; it's a suggested enhancement. If present, it should
    ; return number of bytes copied in RAX.
    MNI Memory.write_bytes dest $label
ENDMACRO
