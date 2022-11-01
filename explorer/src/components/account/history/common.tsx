import { ParsedTransactionWithMeta } from "@solana/web3.js";

export type MintDetails = {
  decimals: number;
  mint: string;
};

export function extractMintDetails(
  transactionWithMeta: ParsedTransactionWithMeta,
  mintMap: Map<string, MintDetails>
) {
  if (transactionWithMeta.meta?.preTokenBalances) {
    transactionWithMeta.meta.preTokenBalances.forEach((balance) => {
      const account =
        transactionWithMeta.transaction.message.accountKeys[
          balance.accountIndex
        ];
      mintMap.set(account.pubkey.toBase58(), {
        decimals: balance.uiTokenAmount.decimals,
        mint: balance.mint,
      });
    });
  }

  if (transactionWithMeta.meta?.postTokenBalances) {
    transactionWithMeta.meta.postTokenBalances.forEach((balance) => {
      const account =
        transactionWithMeta.transaction.message.accountKeys[
          balance.accountIndex
        ];
      mintMap.set(account.pubkey.toBase58(), {
        decimals: balance.uiTokenAmount.decimals,
        mint: balance.mint,
      });
    });
  }
}
