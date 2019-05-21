PHDRS
{
  text PT_LOAD  ;
  rodata PT_LOAD ;
  dynamic PT_DYNAMIC ;
}

SECTIONS
{
  . = SIZEOF_HEADERS;
  .text : { *(.text) } :text
  .rodata : { *(.rodata) } :rodata
  .dynamic : { *(.dynamic) } :dynamic
  .dynsym : { *(.dynsym) } :dynamic
  .dynstr : { *(.dynstr) } :dynamic
  .gnu.hash : { *(.gnu.hash) } :dynamic
  .rel.dyn : { *(.rel.dyn) } :dynamic
  .hash : { *(.hash) } :dynamic
}
