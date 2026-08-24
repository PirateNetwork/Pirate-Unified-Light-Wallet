#ifndef PIRATE_WALLET_SERVICE_H
#define PIRATE_WALLET_SERVICE_H

#pragma once

/* Generated with cbindgen:0.29.3 */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * # Safety
 *
 * `ptr` must be a pointer previously returned by this library via
 * [`pirate_wallet_service_invoke_json`] or another compatible allocator path.
 * It must not be freed more than once and must not be used after this call.
 */
void pirate_wallet_service_free_string(char *ptr);

/**
 * # Safety
 *
 * `request_json` must be a valid, NUL-terminated UTF-8 C string pointer for
 * the duration of this call.
 */
char *pirate_wallet_service_invoke_json(const char *request_json, bool pretty);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* PIRATE_WALLET_SERVICE_H */
