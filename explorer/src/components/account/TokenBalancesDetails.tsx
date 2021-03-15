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
import { SlotRow } from "./TransactionHistoryCardWrapper";
import { Signature } from "components/common/Signature";

export function TokenBalancesDetails({
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
  const detailsList: React.ReactNode[] = [];

  slotRows.forEach(
    ({ slot, signature, blockTime, statusClass, statusText }) => {
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
          <tr key={signature}>
            <td className="w-1">
              <Slot slot={slot} link />
            </td>

            {hasTimestamps && (
              <td className="text-muted">
                {blockTime ? displayTimestamp(blockTime * 1000, true) : "---"}
              </td>
            )}

            <td>
              <span className={`badge badge-soft-${statusClass}`}>
                {statusText}
              </span>
            </td>

            <td className="text-muted">No token balances changed</td>

            <td>
              <span className="badge badge-soft-secondary">0</span>
            </td>

            <td>
              <span className="badge badge-soft-secondary">-</span>
            </td>

            <td>
              <Signature signature={signature} link />
            </td>
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
          <tr key={key}>
            <td className="w-1">
              <Slot slot={slot} link />
            </td>

            {hasTimestamps && (
              <td className="text-muted">
                {blockTime ? displayTimestamp(blockTime * 1000, true) : "---"}
              </td>
            )}

            <td>
              <span className={`badge badge-soft-${statusClass}`}>
                {statusText}
              </span>
            </td>

            <td>
              <Address pubkey={account} link />
            </td>

            <td>
              <BalanceDelta delta={delta} />
            </td>

            <td>
              {balance.uiAmount} {units}
            </td>

            <td>
              <Signature signature={signature} link />
            </td>
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
            {hasTimestamps && <th className="text-muted">Timestamp</th>}
            <th className="text-muted">Result</th>
            <th className="text-muted">Account</th>
            <th className="text-muted">Balance Change</th>
            <th className="text-muted">Post Balance</th>
            <th className="text-muted">Transaction Signature</th>
          </tr>
        </thead>
        <tbody className="list">{detailsList}</tbody>
      </table>
    </div>
  );
}
