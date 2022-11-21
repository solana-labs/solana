#pragma once
/**
 * @brief Solana constants
 */

/**
 * The Solana runtime provides a memory region that is available to programs at
 * a fixed virtual address and length. The builtin functions `sol_calloc` and
 * `sol_free` call into the Solana runtime to allocate from this memory region
 * for heap operations.  Because the memory region is directly available to
 * programs another option is a program can implement their own heap directly on
 * top of that region.  If a program chooses to implement their own heap they
 * should not call the builtin heap functions because they will conflict.
 * `HEAP_START_ADDRESS` and `HEAP_LENGTH` specify the memory region's start
 * virtual address and length.
 */
#define HEAP_START_ADDRESS (uint64_t)0x300000000
#define HEAP_LENGTH (uint64_t)(32 * 1024)
