#ifndef PIRATE_WALLET_SERVICE_H
#define PIRATE_WALLET_SERVICE_H

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

char *pirate_wallet_service_invoke_json(const char *request_json, bool pretty);
void pirate_wallet_service_free_string(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
