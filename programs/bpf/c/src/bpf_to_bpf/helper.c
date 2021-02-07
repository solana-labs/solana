/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <safecoin_sdk.h>
#include "helper.h"

void helper_function(void) {
  safe_log(__FILE__);
}
