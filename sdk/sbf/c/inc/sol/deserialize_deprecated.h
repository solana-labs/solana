#pragma once
/**
 * @brief Solana deprecated BPF loader deserializer to be used when deploying
 * a program with `BPFLoader1111111111111111111111111111111111`
 */

 #include <sol/types.h>
 #include <sol/pubkey.h>
 #include <sol/entrypoint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * De-serializes the input parameters into usable types
 *
 * Use this function to deserialize the buffer passed to the program entrypoint
 * into usable types.  This function does not perform copy deserialization,
 * instead it populates the pointers and lengths in SolAccountInfo and data so
 * that any modification to lamports or account data take place on the original
 * buffer.  Doing so also eliminates the need to serialize back into the buffer
 * at the end of the program.
 *
 * @param input Source buffer containing serialized input parameters
 * @param params Pointer to a SolParameters structure
 * @return Boolean true if successful.
 */
static bool sol_deserialize_deprecated(
  const uint8_t *input,
  SolParameters *params,
  uint64_t ka_num
) {
  if (NULL == input || NULL == params) {
    return false;
  }
  params->ka_num = *(uint64_t *) input;
  input += sizeof(uint64_t);

  for (int i = 0; i < params->ka_num; i++) {
    uint8_t dup_info = input[0];
    input += sizeof(uint8_t);

    if (i >= ka_num) {
      if (dup_info == UINT8_MAX) {
        input += sizeof(uint8_t);
        input += sizeof(uint8_t);
        input += sizeof(SolPubkey);
        input += sizeof(uint64_t);
        input += *(uint64_t *) input;
        input += sizeof(uint64_t);
        input += sizeof(SolPubkey);
        input += sizeof(uint8_t);
        input += sizeof(uint64_t);
      }
      continue;
    }
    if (dup_info == UINT8_MAX) {
      // is signer?
      params->ka[i].is_signer = *(uint8_t *) input != 0;
      input += sizeof(uint8_t);

      // is writable?
      params->ka[i].is_writable = *(uint8_t *) input != 0;
      input += sizeof(uint8_t);

      // key
      params->ka[i].key = (SolPubkey *) input;
      input += sizeof(SolPubkey);

      // lamports
      params->ka[i].lamports = (uint64_t *) input;
      input += sizeof(uint64_t);

      // account data
      params->ka[i].data_len = *(uint64_t *) input;
      input += sizeof(uint64_t);
      params->ka[i].data = (uint8_t *) input;
      input += params->ka[i].data_len;

      // owner
      params->ka[i].owner = (SolPubkey *) input;
      input += sizeof(SolPubkey);

      // executable?
      params->ka[i].executable = *(uint8_t *) input;
      input += sizeof(uint8_t);

      // rent epoch
      params->ka[i].rent_epoch = *(uint64_t *) input;
      input += sizeof(uint64_t);
    } else {
      params->ka[i].is_signer = params->ka[dup_info].is_signer;
      params->ka[i].key = params->ka[dup_info].key;
      params->ka[i].lamports = params->ka[dup_info].lamports;
      params->ka[i].data_len = params->ka[dup_info].data_len;
      params->ka[i].data = params->ka[dup_info].data;
      params->ka[i].owner = params->ka[dup_info].owner;
      params->ka[i].executable = params->ka[dup_info].executable;
      params->ka[i].rent_epoch = params->ka[dup_info].rent_epoch;
    }
  }

  params->data_len = *(uint64_t *) input;
  input += sizeof(uint64_t);
  params->data = input;
  input += params->data_len;

  params->program_id = (SolPubkey *) input;
  input += sizeof(SolPubkey);

  return true;
}

#ifdef __cplusplus
}
#endif

/**@}*/
