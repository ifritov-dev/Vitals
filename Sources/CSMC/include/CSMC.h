#ifndef CSMC_H
#define CSMC_H

#include <stdint.h>

// Minimal AppleSMC user-client bridge.
//
// The wire format is a fixed 80-byte struct whose layout must match the
// driver exactly. Swift cannot guarantee that layout, so every exchange
// happens here and Swift only sees plain values.

// Opens the AppleSMC user client. Returns 0 on success, negative on failure.
int32_t smc_open(void);
void smc_close(void);

// Number of keys the SMC exposes (the "#KEY" value).
int32_t smc_key_count(uint32_t *count);

// Writes the 4-character key at `index` into key[0..3], NUL terminated.
int32_t smc_key_at(uint32_t index, char key[5]);

// Reads a key's declared data size and 4-character type ("flt", "sp78", ...).
int32_t smc_read_keyinfo(const char *key, uint32_t *dataSize, char type[5]);

// Reads a key's raw bytes. `dataSize` must come from smc_read_keyinfo.
// bytes must have room for 32 bytes.
int32_t smc_read_bytes(const char *key, uint32_t dataSize, uint8_t bytes[32]);

// Last SMC-level status byte from the most recent failing call (0 when ok).
uint8_t smc_last_result(void);

#endif
