; threading.inc - helper macros for MASM to call Thread MNI functions
; These macros use MicroV2-style macro syntax (MACRO / ENDMACRO) and
; perform simple, textual argument substitution. They do not themselves
; emit the MNI call; they push arguments in a consistent way so callers
; can invoke the runtime MNI (`MNI Thread.spawn`, etc.) using the
; assembler's MNI encoding.

; Example usage:
;   THREAD_SPAWN worker_entry
;   MNI Thread.spawn        ; assembler-specific MNI encoding
;   ; thread id returned in RAX (per runtime convention)

MACRO THREAD_SPAWN label
    ; push the numeric address of `label` onto the stack
    PUSH $label
ENDMACRO

MACRO THREAD_JOIN tid
    ; push the thread id (numeric) then caller should invoke MNI Thread.join
    PUSH $tid
ENDMACRO

MACRO THREAD_DETACH tid
    PUSH $tid
ENDMACRO

; Notes:
; - Macro parameters are substituted textually. Invoke like: THREAD_SPAWN my_label
; - If you need local labels inside a macro expansion, use the `@@` prefix in
;   your macro body so the assembler will rename them uniquely per expansion.
;   Example (not provided above) `@@loop:` becomes a unique label for each call.

; ------------------------------------------------------------------
; MNI feature list (Thread module)
;
; Implemented in runtime (Rust MNI shims):
; - Thread.spawn <label_addr> [arg_addr]
;     Spawn a new OS thread that starts executing at <label_addr>. Optionally
;     pass a C-string address arg_addr which the new thread can read from memory.
;     Returns a numeric thread id in RAX.
; - Thread.join <thread_id>
;     Block until thread with id returns; returns 0 in RAX on success, -1 on error.
; - Thread.detach <thread_id>
;     Detach background thread, returns 0 on success, -1 on error.
;
; Suggested additions:
; - Thread.spawn_with_stack <label> <stack_size>
;     Allow callers to request custom stack sizes for spawned threads.
; - Thread.sleep <ms>
;     Block current VM thread for the given milliseconds (cooperative sleep).

; Convenience macro that emits the MNI call directly (if assembler supports it):
MACRO SPAWN_AND_CALL label
    THREAD_SPAWN label
    MNI Thread.spawn
ENDMACRO

MACRO JOIN_AND_CALL tid
    THREAD_JOIN tid
    MNI Thread.join
ENDMACRO

MACRO DETACH_AND_CALL tid
    THREAD_DETACH tid
    MNI Thread.detach
ENDMACRO
