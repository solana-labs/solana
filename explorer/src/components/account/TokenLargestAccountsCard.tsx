import React from "react";
import { PublicKey } from "@safecoin/web3.js";
import { LoadingCard } from "components/common/LoadingCard";
import { ErrorCard } from "components/common/ErrorCard";
import { Address } from "components/common/Address";
import {
  useTokenLargestTokens,
  useFetchTokenLargestAccounts,
  TokenAccountBalancePairWithOwner,
} from "providers/mints/largest";
import { FetchStatus } from "providers/cache";
import { TokenRegistry } from "tokenRegistry";
import { useCluster } from "providers/cluster";
import { useMintAccountInfo } from "providers/accounts";
import { normalizeTokenAmount } from "utils";

export function TokenLargestAccountsCard({ pubkey }: { pubkey: PublicKey }) {
  const mintAddress = pubkey.toBase58();
  const mintInfo = useMintAccountInfo(mintAddress);
  const largestAccounts = useTokenLargestTokens(mintAddress);
  const fetchLargestAccounts = useFetchTokenLargestAccounts();
  const refreshLargest = React.useCallback(() => fetchLargestAccounts(pubkey), [
    pubkey,
    fetchLargestAccounts,
  ]);
  const { cluster } = useCluster();
  const unit = TokenRegistry.get(mintAddress, cluster)?.symbol;
  const unitLabel = unit ? `(${unit})` : "";

  React.useEffect(() => {
    if (mintInfo) refreshLargest();
  }, [mintInfo, refreshLargest]);

  // Largest accounts hasn't started fetching
  if (largestAccounts === undefined) return null;

  // This is not a mint account
  if (mintInfo === undefined) return null;

  if (largestAccounts?.data === undefined) {
    if (largestAccounts.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading largest accounts" />;
    }

    return (
      <ErrorCard
        retry={refreshLargest}
        text="Failed to fetch largest accounts"
      />
    );
  } else if (largestAccounts.status === FetchStatus.Fetching) {
    return <LoadingCard message="Refreshing largest accounts" />;
  }

  const accounts = largestAccounts.data.largest;
  if (accounts.length === 0) {
    return <ErrorCard text="No holders found" />;
  }

  // Find largest fixed point in accounts array
  const balanceFixedPoint = accounts.reduce(
    (prev: number, current: TokenAccountBalancePairWithOwner) => {
      const amount = `${current.uiAmount}`;
      const length = amount.length;
      const decimalIndex = amount.indexOf(".");
      if (decimalIndex >= 0 && length - decimalIndex - 1 > prev) {
        return length - decimalIndex - 1;
      }
      return prev;
    },
    0
  );

  const supplyTotal = normalizeTokenAmount(mintInfo.supply, mintInfo.decimals);

  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h4 className="card-header-title">Largest Accounts</h4>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm card-table">
            <thead>
              <tr>
                <th className="text-muted">Rank</th>
                <th className="text-muted">Address</th>
                <th className="text-muted">Owner</th>
                <th className="text-muted text-right">Balance {unitLabel}</th>
                <th className="text-muted text-right">% of Total Supply</th>
              </tr>
            </thead>
            <tbody className="list">
              {accounts.map((account, index) =>
                renderAccountRow(account, index, balanceFixedPoint, supplyTotal)
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}

const renderAccountRow = (
  account: TokenAccountBalancePairWithOwner,
  index: number,
  balanceFixedPoint: number,
  supply: number
) => {
  let percent = "-";
  if (supply > 0) {
    percent = `${((100 * account.uiAmount) / supply).toFixed(3)}%`;

    if (parseFloat(percent) === 0 && account.uiAmount > 0) {
      percent = `~${percent}`;
    }
  }
  return (
    <tr key={index}>
      <td>
        <span className="badge badge-soft-gray badge-pill">{index + 1}</span>
      </td>
      <td className="td">
        <Address pubkey={account.address} link truncate />
      </td>
      <td>
        {account.owner && <Address pubkey={account.owner} link truncate />}
      </td>
      <td className="text-right text-monospace">
        {fixedLocaleNumber(account.uiAmount, balanceFixedPoint)}
      </td>
      <td className="text-right text-monospace">{percent}</td>
    </tr>
  );
};

function fixedLocaleNumber(value: number, fixedPoint: number) {
  const fixed = value.toFixed(fixedPoint);
  const split = fixed.split(".");

  if (fixedPoint < 1) {
    return parseInt(split[0], 10).toLocaleString("en");
  }

  return [parseInt(split[0], 10).toLocaleString("en"), split[1]].join(".");
}
