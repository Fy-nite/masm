masi files have a specific structure:
 
 Header (16 bytes): encodes metadata about the MASI file, such as version, entry point, etc.
 Import Table: lists external dependencies and their addresses within the MASI file.
 Local Variable Table: lists local variables used in the program.
 Label Table: lists labels used for jumps and branches in the code and their address within the file.
 Constant Table: lists constants used in the program.
 Code Section: contains the actual bytecode instructions to be executed.
 Data Section: contains static data used by the program.
 Export Table: lists functions and variables that can be accessed from outside the MASI file.
 Note: The exact structure and size of each section may vary based on the MASI file version and the specific program.
 though, this is subject to change as the MASI format evolves.
 
 another thing to note is that instructions have to also handle # and $ for label addresing and accessing memory.
 
 each instruction is one byte, followed by a byte for if it's a memory access or immediate value, followed by the value itself
 either as a 4 byte integer, reference to a label, register, etc.
 For example:
 mov RAX, 5
 would be encoded as:
 [opcode for mov][0 for immediate][4 byte integer 5]