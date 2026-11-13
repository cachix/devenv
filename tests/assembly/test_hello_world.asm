global _start

section .data
   msg     db "Hello, World!", 10
   len     equ $ - msg

section .text

_start:
	mov rax, 1      ; sys_write
	mov rdi, 1      ; stdout
	mov rsi, msg    ; messagea pointer
	mov rdx, len    ; message length
	syscall

	mov rax, 60     ; sys_exit
	xor rdi, rdi    ; exit code 0
	syscall
