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
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/last_restart_slot.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
u64 sol_get_last_restart_slot();
#else
typedef u64(*sol_get_last_restart_slot_pointer_type)();
static u64 sol_get_last_restart_slot() {
  sol_get_last_restart_slot_pointer_type sol_get_last_restart_slot_pointer = (sol_get_last_restart_slot_pointer_type) 411697201;
  return sol_get_last_restart_slot_pointer();
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
