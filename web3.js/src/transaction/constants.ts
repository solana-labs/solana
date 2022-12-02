/**
 * Maximum over-the-wire size of a Transaction
 *
 * 1280 is IPv6 minimum MTU
 * 40 bytes is the size of the IPv6 header
 * 8 bytes is the size of the fragment header
 */
export const PACKET_DATA_SIZE = 1280 - 40 - 8;

// Maximum over-the-wire size of a large transaction, currently two packets
export const TRANSACTION_PACKET_DATA_SIZE = PACKET_DATA_SIZE * 2;

export const VERSION_PREFIX_MASK = 0x7f;

export const SIGNATURE_LENGTH_IN_BYTES = 64;
