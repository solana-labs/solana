#pragma once

/**
 * This file is part of libsealevel.
 *
 * This is a support header to support variadic-style config arguments, which is currently not supported by cbindgen.
 */

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

void sealevel_config_setopt();

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus
