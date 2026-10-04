ENTRY(_start)
SECTIONS {
  . = 0x100000;
  .text : { *(.text.start) *(.text .text.*) *(.rodata .rodata.*) }
  /DISCARD/ : { *(.data*) *(.bss*) *(.comment) *(.note*) *(.eh_frame*) }
}
