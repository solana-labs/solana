// TODO: These constants should be removed in favor of reading them out of a
// Syscall account

/**
 * @internal
 */
export const NUM_TICKS_PER_SECOND = 160;

/**
 * @internal
 */
export const DEFAULT_TICKS_PER_SLOT = 64;

/**
 * @internal
 */
export const NUM_SLOTS_PER_SECOND =
  NUM_TICKS_PER_SECOND / DEFAULT_TICKS_PER_SLOT;

/**
 * @internal
 */
export const MS_PER_SLOT = 1000 / NUM_SLOTS_PER_SECOND;
