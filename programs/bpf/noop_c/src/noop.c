
//#include <stdint.h>
//#include <stddef.h>

#if 1
// one way to define a helper function is with index as a fixed value
#define BPF_TRACE_PRINTK_IDX 6
static int (*sol_print)(int, int, int, int, int) = (void *)BPF_TRACE_PRINTK_IDX;
#else
// relocation is another option
extern int sol_print(int, int, int, int, int);
#endif

typedef long long unsigned int uint64_t;
typedef long long int int64_t;
typedef unsigned char uint8_t;

typedef enum { false = 0, true } bool;

#define SIZE_PUBKEY 32
typedef struct {
    uint8_t x[SIZE_PUBKEY];
} SolPubkey;

typedef struct {
    SolPubkey *key;
    int64_t* tokens;
    uint64_t userdata_len;
    uint8_t *userdata;
    SolPubkey *program_id;
} SolKeyedAccounts;

// TODO support BPF function calls rather then forcing everything to be inlined
#define SOL_FN_PREFIX __attribute__((always_inline)) static

// TODO move this to a registered helper
SOL_FN_PREFIX void sol_memcpy(void *dst, void *src, int len) {
    for (int i = 0; i < len; i++) {
        *((uint8_t *)dst + i) = *((uint8_t *)src + i);
    }
}

#define sol_panic() _sol_panic(__LINE__)
SOL_FN_PREFIX void _sol_panic(uint64_t line) {
    sol_print(0, 0, 0xFF, 0xFF, line);
    char *pv = (char *)1;
    *pv = 1;
}

SOL_FN_PREFIX int sol_deserialize(uint8_t *src, uint64_t num_ka, SolKeyedAccounts *ka,
                              uint8_t **userdata, uint64_t *userdata_len) {
    if (num_ka != *(uint64_t *)src) {
        return 0;
    }
    src += sizeof(uint64_t);

    // TODO fixed iteration loops ok? unrolled?
    for (int i = 0; i < num_ka; i++) { // TODO this should end up unrolled, confirm
        // key
        ka[i].key = (SolPubkey *)src;
        src += SIZE_PUBKEY;

        // tokens
        ka[i].tokens = (int64_t *)src;
        src += sizeof(int64_t);

        // account userdata
        ka[i].userdata_len = *(uint64_t *)src;
        src += sizeof(uint64_t);
        ka[i].userdata = src;
        src += ka[i].userdata_len;

        // program_id
        ka[i].program_id = (SolPubkey *)src;
        src += SIZE_PUBKEY;
    }
    // tx userdata
    *userdata_len = *(uint64_t *)src;
    src += sizeof(uint64_t);
    *userdata = src;

    return 1;
}


// -- Debug --

SOL_FN_PREFIX void print_key(SolPubkey *key) {
    for (int j = 0; j < SIZE_PUBKEY; j++) {
        sol_print(0, 0, 0, j, key->x[j]);
    }
}

SOL_FN_PREFIX void print_userdata(uint8_t *data, int len) {
    for (int j = 0; j < len; j++) {
        sol_print(0, 0, 0, j, data[j]);
    }
}

SOL_FN_PREFIX void print_params(uint64_t num_ka, SolKeyedAccounts *ka,
                                uint8_t *userdata, uint64_t userdata_len) {
    sol_print(0, 0, 0, 0, num_ka);
    for (int i = 0; i < num_ka; i++) {
        // key
        print_key(ka[i].key);

        // tokens
        sol_print(0, 0, 0, 0, *ka[i].tokens);

        // account userdata
        print_userdata(ka[i].userdata, ka[i].userdata_len);

        // program_id
        print_key(ka[i].program_id);
    }
    // tx userdata
    print_userdata(userdata, userdata_len);
}

// -- Program entrypoint --

uint64_t entrypoint(char *buf) {
    SolKeyedAccounts ka[1];
    uint64_t userdata_len;
    uint8_t *userdata;

    if (1 != sol_deserialize((uint8_t *)buf, 1, ka, &userdata, &userdata_len)) {
        return 0;
    }
    print_params(1, ka, userdata, userdata_len);
    return 1;
}

