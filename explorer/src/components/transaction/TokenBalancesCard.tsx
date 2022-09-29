import React from "react";
import {
  ParsedMessageAccount,
  PublicKey,
  TokenAmount,
  TokenBalance,
} from "@solana/web3.js";
import { BigNumber } from "bignumber.js";
import { Address } from "components/common/Address";
import { BalanceDelta } from "components/common/BalanceDelta";
import { SignatureProps } from "pages/TransactionDetailsPage";
import { useTransactionDetails } from "providers/transactions";
import { useTokenRegistry } from "providers/mints/token-registry";

export type TokenBalanceRow = {
  account: PublicKey;
  mint: string;
  balance: TokenAmount;
  delta: BigNumber;
  accountIndex: number;
};

export function TokenBalancesCard({ signature }: SignatureProps) {
  const details = useTransactionDetails(signature);
  const { tokenRegistry } = useTokenRegistry();

  if (!details) {
    return null;
  }

  const transactionWithMeta = details.data?.transactionWithMeta;
  const preTokenBalances = transactionWithMeta?.meta?.preTokenBalances;
  const postTokenBalances = transactionWithMeta?.meta?.postTokenBalances;
  const accountKeys = transactionWithMeta?.transaction.message.accountKeys;

  if (!preTokenBalances || !postTokenBalances || !accountKeys) {
    return null;
  }

  const rows = generateTokenBalanceRows(
    preTokenBalances,
    postTokenBalances,
    accountKeys
  );

  if (rows.length < 1) {
    return null;
  }

  const accountRows = rows.map(({ account, delta, balance, mint }) => {
    const key = account.toBase58() + mint;
    const units = tokenRegistry.get(mint)?.symbol || "tokens";

    return (
      <tr key={key}>
        <td>
          <Address pubkey={account} link />
        </td>
        <td>
          <Address pubkey={new PublicKey(mint)} link />
        </td>
        <td>
          <BalanceDelta delta={delta} />
        </td>
        <td>
          {balance.uiAmountString} {units}
        </td>
      </tr>
    );
  });

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Token Balances</h3>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Address</th>
              <th className="text-muted">Token</th>
              <th className="text-muted">Change</th>
              <th className="text-muted">Post Balance</th>
            </tr>
          </thead>
          <tbody className="list">{accountRows}</tbody>
        </table>
      </div>
    </div>
  );
}

export function generateTokenBalanceRows(
  preTokenBalances: TokenBalance[],
  postTokenBalances: TokenBalance[],
  accounts: ParsedMessageAccount[]
): TokenBalanceRow[] {
  let preBalanceMap: { [index: number]: TokenBalance } = {};
  let postBalanceMap: { [index: number]: TokenBalance } = {};

  preTokenBalances.forEach(
    (balance) => (preBalanceMap[balance.accountIndex] = balance)
  );
  postTokenBalances.forEach(
    (balance) => (postBalanceMap[balance.accountIndex] = balance)
  );

  // Check if any pre token balances do not have corresponding
  // post token balances. If not, insert a post balance of zero
  // so that the delta is displayed properly
  for (let index in preBalanceMap) {
    const preBalance = preBalanceMap[index];
    if (!postBalanceMap[index]) {
      postBalanceMap[index] = {
        accountIndex: Number(index),
        mint: preBalance.mint,
        uiTokenAmount: {
          amount: "0",
          decimals: preBalance.uiTokenAmount.decimals,
          uiAmount: null,
          uiAmountString: "0",
        },
      };
    }
  }

  let rows: TokenBalanceRow[] = [];

  for (let index in postBalanceMap) {
    const { uiTokenAmount, accountIndex, mint } = postBalanceMap[index];
    const preBalance = preBalanceMap[accountIndex];
    const account = accounts[accountIndex].pubkey;

    if (!uiTokenAmount.uiAmountString) {
      // uiAmount deprecation
      continue;
    }

    // case where mint changes
    if (preBalance && preBalance.mint !== mint) {
      if (!preBalance.uiTokenAmount.uiAmountString) {
        // uiAmount deprecation
        continue;
      }

      rows.push({
        account: accounts[accountIndex].pubkey,
        accountIndex,
        balance: {
          decimals: preBalance.uiTokenAmount.decimals,
          amount: "0",
          uiAmount: 0,
        },
        delta: new BigNumber(-preBalance.uiTokenAmount.uiAmountString),
        mint: preBalance.mint,
      });

      rows.push({
        account: accounts[accountIndex].pubkey,
        accountIndex,
        balance: uiTokenAmount,
        delta: new BigNumber(uiTokenAmount.uiAmountString),
        mint: mint,
      });
      continue;
    }

    let delta;

    if (preBalance) {
      if (!preBalance.uiTokenAmount.uiAmountString) {
        // uiAmount deprecation
        continue;
      }

      delta = new BigNumber(uiTokenAmount.uiAmountString).minus(
        preBalance.uiTokenAmount.uiAmountString
      );
    } else {
      delta = new BigNumber(uiTokenAmount.uiAmountString);
    }

    rows.push({
      account,
      mint,
      balance: uiTokenAmount,
      delta,
      accountIndex,
    });
  }

  return rows.sort((a, b) => a.accountIndex - b.accountIndex);
}
