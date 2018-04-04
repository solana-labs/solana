#include <stdint.h>
#include <assert.h>
#include <stdio.h>

#define PACKET_SIZE 288
#define PACKET_DATA_SIZE 256
union Packet {
    char data[PACKET_DATA_SIZE];
    char total[PACKET_SIZE];
};

struct Elems {
    union Packet *packet;
    uint32_t len;
};

int ed25519_verify_many(                                                    
    const struct Elems *vecs,                                                    
    uint32_t num,
    uint32_t message_size,
    uint32_t public_key_offset,
    uint32_t signature_offset,
    uint32_t signed_message_offset,
    uint32_t signed_message_len_offset,
    uint8_t *out
) {
    int i, p = 0;
    assert(num > 0);
    for(i = 0; i < num; ++i) {
        int j;
        assert(vecs[i].len > 0);
        assert(message_size == PACKET_SIZE);
        assert(signed_message_len_offset == PACKET_DATA_SIZE);
        for(j = 0; j < vecs[i].len; ++j) { 
            uint32_t *len = (uint32_t*)&vecs[i].packet[j].total[signed_message_len_offset];
            assert(*len <= PACKET_DATA_SIZE);
            p += 1;
            if(public_key_offset > *len - 32) {
                continue;
            }
            if(signature_offset > *len - 64) {
                continue;
            }
            if(signed_message_offset > *len) {
                continue;
            }
            out[p - 1] = 1;
        }
    }
    return 0; 
}
