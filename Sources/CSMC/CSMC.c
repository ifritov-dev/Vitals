#include "CSMC.h"

#include <IOKit/IOKitLib.h>
#include <pthread.h>
#include <string.h>

// The kernel expects exactly this struct: key, version, power-limit block,
// key info, result/status/data8, data32, then 32 bytes of payload.
typedef struct {
    UInt8 major;
    UInt8 minor;
    UInt8 build;
    UInt8 reserved;
    UInt16 release;
} SMCKeyData_vers_t;

typedef struct {
    UInt16 version;
    UInt16 length;
    UInt32 cpuPLimit;
    UInt32 gpuPLimit;
    UInt32 memPLimit;
} SMCKeyData_pLimitData_t;

typedef struct {
    UInt32 dataSize;
    UInt32 dataType;
    UInt8 dataAttributes;
} SMCKeyData_keyInfo_t;

typedef struct {
    UInt32 key;
    SMCKeyData_vers_t vers;
    SMCKeyData_pLimitData_t pLimitData;
    SMCKeyData_keyInfo_t keyInfo;
    UInt8 result;
    UInt8 status;
    UInt8 data8;
    UInt32 data32;
    UInt8 bytes[32];
} SMCKeyData_t;

enum { SMC_KERNEL_INDEX = 2 };
enum { SMC_CMD_READ_BYTES = 5, SMC_CMD_READ_INDEX = 8, SMC_CMD_READ_KEYINFO = 9 };

static io_connect_t g_conn = 0;
static int g_open = 0;
static uint8_t g_last_result = 0;
static pthread_mutex_t g_lock = PTHREAD_MUTEX_INITIALIZER;

static uint32_t fourcc(const char *key) {
    return ((uint32_t)(uint8_t)key[0] << 24) | ((uint32_t)(uint8_t)key[1] << 16) |
           ((uint32_t)(uint8_t)key[2] << 8) | (uint32_t)(uint8_t)key[3];
}

static kern_return_t smc_call(SMCKeyData_t *input, SMCKeyData_t *output) {
    size_t out_size = sizeof(SMCKeyData_t);
    return IOConnectCallStructMethod(g_conn, SMC_KERNEL_INDEX, input, sizeof(SMCKeyData_t), output,
                                     &out_size);
}

int32_t smc_open(void) {
    pthread_mutex_lock(&g_lock);
    if (g_open) {
        pthread_mutex_unlock(&g_lock);
        return 0;
    }
    io_service_t service =
        IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("AppleSMC"));
    if (!service) {
        pthread_mutex_unlock(&g_lock);
        return -1;
    }
    kern_return_t kr = IOServiceOpen(service, mach_task_self(), 0, &g_conn);
    IOObjectRelease(service);
    if (kr != KERN_SUCCESS) {
        g_conn = 0;
        pthread_mutex_unlock(&g_lock);
        return -2;
    }
    g_open = 1;
    pthread_mutex_unlock(&g_lock);
    return 0;
}

void smc_close(void) {
    pthread_mutex_lock(&g_lock);
    if (g_open) {
        IOServiceClose(g_conn);
        g_conn = 0;
        g_open = 0;
    }
    pthread_mutex_unlock(&g_lock);
}

uint8_t smc_last_result(void) { return g_last_result; }

int32_t smc_read_keyinfo(const char *key, uint32_t *dataSize, char type[5]) {
    if (!g_open || strlen(key) != 4) return -1;
    pthread_mutex_lock(&g_lock);
    SMCKeyData_t in, out;
    memset(&in, 0, sizeof in);
    memset(&out, 0, sizeof out);
    in.key = fourcc(key);
    in.data8 = SMC_CMD_READ_KEYINFO;
    kern_return_t kr = smc_call(&in, &out);
    if (kr != KERN_SUCCESS || out.result != 0) {
        g_last_result = out.result;
        pthread_mutex_unlock(&g_lock);
        return -2;
    }
    *dataSize = out.keyInfo.dataSize;
    uint32_t dt = out.keyInfo.dataType;
    type[0] = (char)((dt >> 24) & 0xff);
    type[1] = (char)((dt >> 16) & 0xff);
    type[2] = (char)((dt >> 8) & 0xff);
    type[3] = (char)(dt & 0xff);
    type[4] = 0;
    pthread_mutex_unlock(&g_lock);
    return 0;
}

int32_t smc_read_bytes(const char *key, uint32_t dataSize, uint8_t bytes[32]) {
    if (!g_open || strlen(key) != 4) return -1;
    if (dataSize == 0 || dataSize > 32) return -1;
    pthread_mutex_lock(&g_lock);
    SMCKeyData_t in, out;
    memset(&in, 0, sizeof in);
    memset(&out, 0, sizeof out);
    in.key = fourcc(key);
    in.data8 = SMC_CMD_READ_BYTES;
    in.keyInfo.dataSize = dataSize;
    kern_return_t kr = smc_call(&in, &out);
    if (kr != KERN_SUCCESS || out.result != 0) {
        g_last_result = out.result;
        pthread_mutex_unlock(&g_lock);
        return -2;
    }
    memcpy(bytes, out.bytes, dataSize);
    pthread_mutex_unlock(&g_lock);
    return 0;
}

int32_t smc_key_count(uint32_t *count) {
    uint32_t size = 0;
    char type[5] = {0};
    if (smc_read_keyinfo("#KEY", &size, type) != 0) return -1;
    uint8_t bytes[32];
    memset(bytes, 0, sizeof bytes);
    if (smc_read_bytes("#KEY", size, bytes) != 0) return -1;
    *count = ((uint32_t)bytes[0] << 24) | ((uint32_t)bytes[1] << 16) | ((uint32_t)bytes[2] << 8) |
             (uint32_t)bytes[3];
    return 0;
}

int32_t smc_key_at(uint32_t index, char key[5]) {
    if (!g_open) return -1;
    pthread_mutex_lock(&g_lock);
    SMCKeyData_t in, out;
    memset(&in, 0, sizeof in);
    memset(&out, 0, sizeof out);
    in.data8 = SMC_CMD_READ_INDEX;
    in.data32 = index;
    kern_return_t kr = smc_call(&in, &out);
    if (kr != KERN_SUCCESS || out.result != 0) {
        g_last_result = out.result;
        pthread_mutex_unlock(&g_lock);
        return -2;
    }
    uint32_t k = out.key;
    key[0] = (char)((k >> 24) & 0xff);
    key[1] = (char)((k >> 16) & 0xff);
    key[2] = (char)((k >> 8) & 0xff);
    key[3] = (char)(k & 0xff);
    key[4] = 0;
    pthread_mutex_unlock(&g_lock);
    return 0;
}
