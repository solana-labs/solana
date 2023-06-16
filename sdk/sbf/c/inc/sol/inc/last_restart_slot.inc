#pragma once
/**
 * @brief Solana Last Restart Slot system call
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Get Last Restart Slot
 */
@SYSCALL u64 sol_get_last_restart_slot(uint8_t *result);

#ifdef __cplusplus
}
#endif

/**@}*/
