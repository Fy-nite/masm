; threading_examples.inc - example usage of threading.inc macros
; Example: spawn a thread at label worker and join it

LBL main
    ; prepare any shared memory/state here
    ; spawn worker thread at label 'worker'
    ; assembler must encode the label's address as an immediate for MNI - the following is illustrative
    THREAD_SPAWN worker
    ; assume spawn returns thread id in RAX; store in R1
    MOV R1 RAX

    ; do main-thread work here

    ; join worker
    THREAD_JOIN R1
    ; join result in RAX (1 if success)
    CMP RAX 1
    JE #joined_ok
    HLT

#joined_ok:
    ; continue
    HLT

LBL worker
    ; do worker tasks; write to shared memory using Memory.allocate and read/write MNI helpers
    ; ...
    HLT
