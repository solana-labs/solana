import React from "react";
import { PublicKey } from "@solana/web3.js";
import { useFetchRewards, useRewards } from "providers/accounts/rewards";
import { LoadingCard } from "components/common/LoadingCard";
import { FetchStatus } from "providers/cache";
import { ErrorCard } from "components/common/ErrorCard";
import { Slot } from "components/common/Slot";
import { lamportsToSolString } from "utils";

export function RewardsCard({ pubkey }: { pubkey: PublicKey }) {
  const address = React.useMemo(() => pubkey.toBase58(), [pubkey]);
  const rewards = useRewards(address);
  const fetchRewards = useFetchRewards();
  const loadMore = () => fetchRewards(pubkey);
  const refresh = () => fetchRewards(pubkey);

  React.useEffect(() => {
    if (!rewards) {
      fetchRewards(pubkey);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (!rewards) {
    return null;
  }

  if (rewards?.data === undefined) {
    if (rewards.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading rewards" />;
    }

    return <ErrorCard retry={refresh} text="Failed to fetch rewards" />;
  }

  const rewardsList = rewards.data.rewards.map((reward) => {
    if (!reward) {
      return null;
    }

    return (
      <tr key={reward.epoch}>
        <td>{reward.epoch}</td>
        <td>
          <Slot slot={reward.effectiveSlot} link />
        </td>
        <td>{lamportsToSolString(reward.amount)}</td>
        <td>{lamportsToSolString(reward.postBalance)}</td>
      </tr>
    );
  });

  const { foundOldest } = rewards.data;
  const fetching = rewards.status === FetchStatus.Fetching;
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Rewards</h3>
            </div>
          </div>
        </div>

        <div className="table-responsive mb-0">
          <table className="table table-sm table-nowrap card-table">
            <thead>
              <tr>
                <th className="w-1 text-muted">Epoch</th>
                <th className="text-muted">Effective Slot</th>
                <th className="text-muted">Reward Amount</th>
                <th className="text-muted">Post Balance</th>
              </tr>
            </thead>
            <tbody className="list">{rewardsList}</tbody>
          </table>
        </div>

        <div className="card-footer">
          {foundOldest ? (
            <div className="text-muted text-center">
              Fetched full reward history
            </div>
          ) : (
            <button
              className="btn btn-primary w-100"
              onClick={() => loadMore()}
              disabled={fetching}
            >
              {fetching ? (
                <>
                  <span className="spinner-grow spinner-grow-sm mr-2"></span>
                  Loading
                </>
              ) : (
                "Load More"
              )}
            </button>
          )}
        </div>
      </div>
    </>
  );
}
