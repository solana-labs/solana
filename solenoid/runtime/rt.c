#include "utils.h"

extern long sp;
extern long pc;
extern unsigned char stack[];
extern unsigned char mem[];
extern void contract_constructor(
    unsigned char* tx,
    long tx_sz,
    long* ret_offset,
    long* ret_len,
    char* storage
);
extern void contract_runtime(
    unsigned char* tx,
    long tx_sz,
    long* ret_offset,
    long* ret_len,
    char* storage
);
extern void abi_set_0(char* tx, int* tx_len, char* x);

int occupancy = 1;
unsigned char storage[1024*64];

/* overwrite key */
void sload(char* st, char* key) {
    // printf("sload called\n");
    for (int i = 0; i < 1024 * 64; i += 64) {
        if (cmp(st + i, key)) {
            cpy(key, st+i+32);
            break;
        }
    }
}

void sstore(char* st, char* key, char* val) {
    // printf("sstore called\n");
    if (occupancy == 1024) { return; }

    int found = 0;
    int loc = occupancy * 64;

    for (int i = 0; i < 1024 * 64; i += 64) {
        if (cmp(st + i, key)) {
            found = 1;
            loc = i;
            break;
        }
    }

    if (!found) {
        occupancy++;
        cpy(st + loc, key);
    }
    for (int i = 0; i < 32; i++) {
        cpy(st + loc+32, val);
    }
}

void dump_storage() {
    for (int i = 0; i < occupancy * 64; i += 64) {
        for (int j = i + 31; j >= i; j--) {
            unsigned char k = storage[j];
            printf("%02X", k);
        }
        printf(" : ");
        for (int j = i + 63; j >= i+32; j--) {
            unsigned char k = storage[j];
            printf("%02X", k);
        }
        printf("\n");
    }
    printf("\n");
}


void dump_stack(char* label) {
    printf("----%s----\nstack:(%ld)@%ld\n", label, sp, pc);
    int top = 10;
    int size = top > 0 ? top * 32 : (1024 * 256 / 8);
    for (int i = 0; i < size; i += 32) {
        char* arrow = (sp * 32) == i ? " ->" : "   ";
        printf("%s@%04x ", arrow, i);
        if ((sp * 32) == i) break;
        for (int j = i + 31; j >= i; j--) {
            unsigned char k = stack[j];
            printf("%02X", k);
        }
        printf("\n");
    }
    printf("\n");

    printf(" mem:\n");
    size = top > 0 ? top * 32 : (1024 * 256 / 8);
    for (int i = 0; i < size; i += 32) {
        printf(" %04x ", i);
        for (int j = i + 31; j >= i; j--) {
            unsigned char k = mem[j];
            printf("%02X", k);
        }
        printf("\n");
    }
    printf("\n");
}

void swap_endianness(char* i) {
    inplace_reverse(i, 32);
}

int main() {
    long offset = 0, length = 0;

    contract_constructor(NULL, 0, &offset, &length, (char*)storage);
    printf("return offset: %ld\nreturn length: %ld\n", offset, length);
    printf("storage occupancy: %d\n", occupancy);
    dump_storage();
    offset = 0; length = 0;


    unsigned char tx[1024] = {0};
    int sz = 0;
    unsigned char num[32] = {0};
    abi_set_0((char*)tx, &sz, pad_int((char*)num, 1));
    printf("%d\n", sz);

    for (int i = 0; i < 10; i++) {
        contract_runtime(tx, sz, &offset, &length, (char*)storage);
        printf("return offset: %ld\nreturn length: %ld\n", offset, length);
        printf("storage occupancy: %d\n", occupancy);
        dump_storage();
        offset = 0; length = 0;
    }
}