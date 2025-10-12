; stdio.inc - convenience macros for input/output (MicroV2 MACRO syntax)
; Usage:
;   %include "stdio.inc"
;   PRINT_STR msg      ; prints null-terminated string at label `msg`
;   PRINT_NUM reg      ; prints numeric register value
;   READ_LINE buf      ; reads a line from stdin into buffer at address `buf`

; These macros perform simple textual substitution and rely on the runtime
; instructions `OUT` and `IN` described by MicroV2. They do not allocate buffers.

MACRO PRINT_STR label
    OUT 1 $label
ENDMACRO

MACRO PRINT_NUM reg
    OUT 1 reg
ENDMACRO

MACRO READ_LINE dest
    IN $dest
ENDMACRO

; ------------------------------------------------------------------
; MNI feature list (IO module)
;
; Current runtime supports simple OUT/IN instructions and a minimal `Syscall` emulation.
; Useful MNI functions that could be added:
; - IO.print_str <addr>
;     Print the NUL-terminated string at <addr> to stdout (convenience wrapper).
; - IO.print_bytes <addr> <len> <fd>
;     Write raw bytes to file descriptor `fd` (1 = stdout, 2 = stderr).
; - IO.read_line <dest_addr> <max_len>
;     Read a line from stdin into `dest_addr` up to `max_len` bytes (NUL-terminated), return bytes read in RAX.
;
; Convenience MNI wrappers (emit MNI if assembler supports it):
MACRO IO_PRINT label
    MNI IO.print_str $label
ENDMACRO

MACRO IO_READ_LINE dest maxlen
    MNI IO.read_line $dest maxlen
ENDMACRO
