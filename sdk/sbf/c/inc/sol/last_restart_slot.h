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
u64 sol_get_last_restart_slot(uint8_t *result);
#else
typedef u64(*sol_get_last_restart_slot_pointer_type)(uint8_t *result);
static u64 sol_get_last_restart_slot(uint8_t *result arg1) {
  sol_get_last_restart_slot_pointer_type sol_get_last_restart_slot_pointer = (sol_get_last_restart_slot_pointer_type) 411697201;
  return sol_get_last_restart_slot_pointer(arg1);
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
