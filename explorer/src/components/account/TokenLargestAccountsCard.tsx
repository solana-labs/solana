import React from "react";
import { PublicKey, TokenAccountBalancePair } from "@solana/web3.js";
import { LoadingCard } from "components/common/LoadingCard";
import { ErrorCard } from "components/common/ErrorCard";
import { Address } from "components/common/Address";
import {
  useTokenLargestTokens,
  useFetchTokenLargestAccounts,
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
  const refreshLargest = () => fetchLargestAccounts(pubkey);
  const { cluster } = useCluster();
  const unit = TokenRegistry.get(mintAddress, cluster)?.symbol;
  const unitLabel = unit ? `(${unit})` : "";

  React.useEffect(() => {
    if (!largestAccounts) refreshLargest();
  }, [mintAddress]); // eslint-disable-line react-hooks/exhaustive-deps

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
  }

  const accounts = largestAccounts.data.largest;
  if (accounts.length === 0) {
    return <ErrorCard text="No holders found" />;
  }

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
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="text-muted">Rank</th>
                <th className="text-muted">Address</th>
                <th className="text-muted text-right">Balance {unitLabel}</th>
                <th className="text-muted text-right">% of Total Supply</th>
              </tr>
            </thead>
            <tbody className="list">
              {accounts.map((account, index) =>
                renderAccountRow(account, index, supplyTotal)
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}

const renderAccountRow = (
  account: TokenAccountBalancePair,
  index: number,
  supply: number
) => {
  let percent = "-";
  if (supply > 0) {
    percent = `${((100 * account.uiAmount) / supply).toFixed(3)}%`;
  }
  return (
    <tr key={index}>
      <td>
        <span className="badge badge-soft-gray badge-pill">{index + 1}</span>
      </td>
      <td>
        <Address pubkey={account.address} link />
      </td>
      <td className="text-right">{account.uiAmount}</td>
      <td className="text-right">{percent}</td>
    </tr>
  );
};
