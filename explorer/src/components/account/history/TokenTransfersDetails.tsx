import React from "react";
import { PublicKey } from "@solana/web3.js";
import { useAccountHistory } from "providers/accounts";
import { Slot } from "components/common/Slot";
import { displayTimestamp } from "utils/date";
import {
  useDetailedAccountHistory,
  useFetchDetailedAccountHistory,
} from "providers/accounts/detailed-history";
import { generateTokenBalanceRows } from "components/transaction/TokenBalancesCard";
import { BalanceDelta } from "components/common/BalanceDelta";
import { Address } from "components/common/Address";
import { useTokenRegistry } from "providers/mints/token-registry";
import { SlotRow } from "../TransactionHistoryCardWrapper";
import { Signature } from "components/common/Signature";

export function TransfersDetails({
  pubkey,
  slotRows,
}: {
  pubkey: PublicKey;
  slotRows: SlotRow[];
}) {
  const address = pubkey.toBase58();
  const history = useAccountHistory(address);
  const fetchDetailedAccountHistory = useFetchDetailedAccountHistory(pubkey);
  const detailedHistoryMap = useDetailedAccountHistory(address);
  const { tokenRegistry } = useTokenRegistry();

  React.useEffect(() => {
    if (history?.data?.fetched) {
      fetchDetailedAccountHistory(history.data.fetched);
    }
  }, [history]); // eslint-disable-line react-hooks/exhaustive-deps

  const hasTimestamps = !!slotRows.find((element) => !!element.blockTime);
  const hasFailed = !!slotRows.find((element) => element.failed);
  const detailsList: React.ReactNode[] = [];

  slotRows.forEach(
    ({ slot, signature, blockTime, failed }) => {
      const parsed = detailedHistoryMap.get(signature);
      if (!parsed) return;

      const preTokenBalances = parsed.meta?.preTokenBalances;
      const postTokenBalances = parsed.meta?.postTokenBalances;
      const accountKeys = parsed.transaction.message.accountKeys;

      if (!preTokenBalances || !postTokenBalances || !accountKeys) {
        return;
      }

      const rows = generateTokenBalanceRows(
        preTokenBalances,
        postTokenBalances,
        accountKeys
      );

      if (rows.length < 1) {
        detailsList.push(
          <tr key={signature}  className={`${failed && "transaction-failed"}`}
          title={`${failed && "Transaction Failed"}`}>
            <td className="w-1">
              <Slot slot={slot} link />
            </td>
            <td className="text-muted">No token balances changed</td>

            <td>
              <span className="badge badge-soft-secondary">0</span>
            </td>

            <td>
              <span className="badge badge-soft-secondary">-</span>
            </td>

            <td>
              <Signature signature={signature} link truncate />
            </td>

            {hasTimestamps && (
              <td className="text-muted">
                {blockTime ? displayTimestamp(blockTime * 1000, true) : "---"}
              </td>
            )}
          </tr>
        );

        return;
      }

      rows.forEach(({ account, delta, balance, mint }) => {
        if (mint !== address) {
          return;
        }

        const key = signature + account.toBase58() + mint;
        const units = tokenRegistry.get(mint)?.symbol || "tokens";

        detailsList.push(
          <tr key={key}  className={`${failed && "transaction-failed"}`}
          title={`${failed && "Transaction Failed"}`}>
            <td className="w-1">
              <Slot slot={slot} link />
            </td>

            <td>
              <Address pubkey={account} link truncate />
            </td>

            <td>
              <BalanceDelta delta={delta} />
            </td>

            <td>
              {balance.uiAmountString} {units}
            </td>

            <td>
              <Signature signature={signature} link truncate />
            </td>

            {hasTimestamps && (
              <td className="text-muted">
                {blockTime ? displayTimestamp(blockTime * 1000, true) : "---"}
              </td>
            )}
          </tr>
        );
      });
    }
  );

  return (
    <div className="table-responsive mb-0">
      <table className="table table-sm table-nowrap card-table">
        <thead>
          <tr>
            <th className="text-muted w-1">Slot</th>
            <th className="text-muted">Account</th>
            <th className="text-muted">Balance Change</th>
            <th className="text-muted">Post Balance</th>
            <th className="text-muted">Transaction Signature</th>
            {hasTimestamps && <th className="text-muted">Timestamp</th>}
          </tr>
        </thead>
        <tbody className="list">{detailsList}</tbody>
      </table>
    </div>
  );
}
