.section .rodata
message: .ascii "Hello, World!\n" 
.equ message_len, 13

.section .text
.globl _start

_start:
    # Load the file descriptor (stdout) into register a0
    li a0, 1
    # Load the address of our message into a1
    la a1, message
    # Load the length of our message into a2
    li a2, message_len
    # Make the system call to write to stdout
    li a7, 64  # sys_write syscall number 
    ecall

    # Exit the program
    li a0, 0  # Exit code
    li a7, 93 # sys_exit syscall number 
    ecall
