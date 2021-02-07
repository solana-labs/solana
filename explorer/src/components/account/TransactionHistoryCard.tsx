import React from "react";
import { PublicKey } from "@safecoin/web3.js";
import { FetchStatus } from "providers/cache";
import { useAccountInfo, useAccountHistory } from "providers/accounts";
import { useFetchAccountHistory } from "providers/accounts/history";
import { Signature } from "components/common/Signature";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { Slot } from "components/common/Slot";
import { displayTimestamp } from "utils/date";

export function TransactionHistoryCard({ pubkey }: { pubkey: PublicKey }) {
  const address = pubkey.toBase58();
  const info = useAccountInfo(address);
  const history = useAccountHistory(address);
  const fetchAccountHistory = useFetchAccountHistory();
  const refresh = () => fetchAccountHistory(pubkey, true);
  const loadMore = () => fetchAccountHistory(pubkey);

  React.useEffect(() => {
    if (!history) refresh();
  }, [address]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!history || info?.data === undefined) {
    return null;
  }

  if (history?.data === undefined) {
    if (history.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading history" />;
    }

    return (
      <ErrorCard retry={refresh} text="Failed to fetch transaction history" />
    );
  }

  const transactions = history.data.fetched;
  if (transactions.length === 0) {
    if (history.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading history" />;
    }
    return (
      <ErrorCard
        retry={loadMore}
        retryText="Try again"
        text="No transaction history found"
      />
    );
  }

  const hasTimestamps = !!transactions.find((element) => !!element.blockTime);
  const detailsList: React.ReactNode[] = [];
  for (var i = 0; i < transactions.length; i++) {
    const slot = transactions[i].slot;
    const slotTransactions = [transactions[i]];
    while (i + 1 < transactions.length) {
      const nextSlot = transactions[i + 1].slot;
      if (nextSlot !== slot) break;
      slotTransactions.push(transactions[++i]);
    }

    slotTransactions.forEach(({ signature, err, blockTime }) => {
      let statusText;
      let statusClass;
      if (err) {
        statusClass = "warning";
        statusText = "Failed";
      } else {
        statusClass = "success";
        statusText = "Success";
      }

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

          <td>
            <Signature signature={signature} link />
          </td>
        </tr>
      );
    });
  }

  const fetching = history.status === FetchStatus.Fetching;
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Transaction History</h3>
        <button
          className="btn btn-white btn-sm"
          disabled={fetching}
          onClick={refresh}
        >
          {fetching ? (
            <>
              <span className="spinner-grow spinner-grow-sm mr-2"></span>
              Loading
            </>
          ) : (
            <>
              <span className="fe fe-refresh-cw mr-2"></span>
              Refresh
            </>
          )}
        </button>
      </div>

      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted w-1">Slot</th>
              {hasTimestamps && <th className="text-muted">Timestamp</th>}
              <th className="text-muted">Result</th>
              <th className="text-muted">Transaction Signature</th>
            </tr>
          </thead>
          <tbody className="list">{detailsList}</tbody>
        </table>
      </div>

      <div className="card-footer">
        {history.data.foundOldest ? (
          <div className="text-muted text-center">Fetched full history</div>
        ) : (
          <button
            className="btn btn-primary w-100"
            onClick={loadMore}
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
  );
}
