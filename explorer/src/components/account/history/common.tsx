import { ParsedConfirmedTransaction } from "@solana/web3.js";

export type MintDetails = {
  decimals: number;
  mint: string;
};

export function extractMintDetails(
  parsedTransaction: ParsedConfirmedTransaction,
  mintMap: Map<string, MintDetails>
) {
  if (parsedTransaction.meta?.preTokenBalances) {
    parsedTransaction.meta.preTokenBalances.forEach((balance) => {
      const account =
        parsedTransaction.transaction.message.accountKeys[balance.accountIndex];
      mintMap.set(account.pubkey.toBase58(), {
        decimals: balance.uiTokenAmount.decimals,
        mint: balance.mint,
      });
    });
  }

  if (parsedTransaction.meta?.postTokenBalances) {
    parsedTransaction.meta.postTokenBalances.forEach((balance) => {
      const account =
        parsedTransaction.transaction.message.accountKeys[balance.accountIndex];
      mintMap.set(account.pubkey.toBase58(), {
        decimals: balance.uiTokenAmount.decimals,
        mint: balance.mint,
      });
    });
  }
}
