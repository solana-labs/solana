export class SendTransactionError extends Error {
  logs: string[] | undefined;

  constructor(message: string, logs?: string[]) {
    super(message);

    this.logs = logs;
  }
}

// Keep in sync with client/src/rpc_custom_errors.rs
// Typescript `enums` thwart tree-shaking. See https://bargsten.org/jsts/enums/
export const SolanaJSONRPCErrorCode = {
  JSON_RPC_SERVER_ERROR_BLOCK_CLEANED_UP: -32001,
  JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE: -32002,
  JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE: -32003,
  JSON_RPC_SERVER_ERROR_BLOCK_NOT_AVAILABLE: -32004,
  JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY: -32005,
  JSON_RPC_SERVER_ERROR_TRANSACTION_PRECOMPILE_VERIFICATION_FAILURE: -32006,
  JSON_RPC_SERVER_ERROR_SLOT_SKIPPED: -32007,
  JSON_RPC_SERVER_ERROR_NO_SNAPSHOT: -32008,
  JSON_RPC_SERVER_ERROR_LONG_TERM_STORAGE_SLOT_SKIPPED: -32009,
  JSON_RPC_SERVER_ERROR_KEY_EXCLUDED_FROM_SECONDARY_INDEX: -32010,
  JSON_RPC_SERVER_ERROR_TRANSACTION_HISTORY_NOT_AVAILABLE: -32011,
  JSON_RPC_SCAN_ERROR: -32012,
  JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_LEN_MISMATCH: -32013,
  JSON_RPC_SERVER_ERROR_BLOCK_STATUS_NOT_AVAILABLE_YET: -32014,
  JSON_RPC_SERVER_ERROR_UNSUPPORTED_TRANSACTION_VERSION: -32015,
  JSON_RPC_SERVER_ERROR_MIN_CONTEXT_SLOT_NOT_REACHED: -32016,
} as const;
export type SolanaJSONRPCErrorCodeEnum =
  typeof SolanaJSONRPCErrorCode[keyof typeof SolanaJSONRPCErrorCode];

export class SolanaJSONRPCError extends Error {
  code: SolanaJSONRPCErrorCodeEnum | unknown;
  data?: any;
  constructor(
    {
      code,
      message,
      data,
    }: Readonly<{code: unknown; message: string; data?: any}>,
    customMessage?: string,
  ) {
    super(customMessage != null ? `${customMessage}: ${message}` : message);
    this.code = code;
    this.data = data;
    this.name = 'SolanaJSONRPCError';
  }
}
