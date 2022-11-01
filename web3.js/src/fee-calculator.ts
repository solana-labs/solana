import * as BufferLayout from '@solana/buffer-layout';

/**
 * https://github.com/solana-labs/solana/blob/90bedd7e067b5b8f3ddbb45da00a4e9cabb22c62/sdk/src/fee_calculator.rs#L7-L11
 *
 * @internal
 */
export const FeeCalculatorLayout = BufferLayout.nu64('lamportsPerSignature');

/**
 * Calculator for transaction fees.
 *
 * @deprecated Deprecated since Solana v1.8.0.
 */
export interface FeeCalculator {
  /** Cost in lamports to validate a signature. */
  lamportsPerSignature: number;
}
