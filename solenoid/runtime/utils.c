#include "utils.h"

void inplace_reverse(char* str, uint16_t len)
{
  if (str)
  {
    char * end = str + len - 1;

    // swap the values in the two given variables
    // XXX: fails when a and b refer to same memory location
#   define XOR_SWAP(a,b) do\
    {\
      a ^= b;\
      b ^= a;\
      a ^= b;\
    } while (0)

    // walk inwards from both ends of the string,
    // swapping until we get to the middle
    while (str < end)
    {
      XOR_SWAP(*str, *end);
      str++;
      end--;
    }
#   undef XOR_SWAP
  }
}

char* pad_int(char* out, int x) {
    out[31] = x & 0xff;
    out[30] = (x>>8)  & 0xff;
    out[29] = (x>>16) & 0xff;
    out[28] = (x>>24) & 0xff;
    return out;
}

int cmp(char* a, char* b) {
    for (int i = 0; i < 32; i++) {
        if (a[i] != b[i]) return 0;
    }
    return 1;
}


void cpy(char* a, char* b) {
    for (int i = 0; i < 32; i++) {
        a[i] = b[i];
    }
}

void prt(char* a) {
    for (int i = 0; i < 32; i++) {
        printf("%02X", a[i]);
    }
}
