ENTRY(_start)
SECTIONS {
  . = 0x100000;
  .text : { *(.text.start) *(.text .text.*) *(.rodata .rodata.*) }
  . = 0x10000000;
  .bss (NOLOAD) : { *(.bss .bss.* COMMON) }
  /DISCARD/ : { *(.data*) *(.comment) *(.note*) *(.eh_frame*) }
}
