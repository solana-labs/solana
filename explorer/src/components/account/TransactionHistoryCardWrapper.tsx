import React from "react";
import { ConfirmedSignatureInfo, PublicKey } from "@solana/web3.js";
import { FetchStatus } from "providers/cache";
import { useAccountInfo, useAccountHistory } from "providers/accounts";
import { useFetchAccountHistory } from "providers/accounts/history";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { MoreTabs } from "pages/AccountDetailsPage";
import { TransactionHistoryDetails } from "./history/TransactionHistoryDetails";
import { TransfersDetails } from "./history/TokenTransfersDetails";
import { TokenInstructionsDetails } from "./history/TokenInstructionsDetails";

export type SlotRow = {
  slot: number;
  signature: string;
  err: object | null;
  blockTime: number | null | undefined;
  failed: boolean;
  signatureInfo: ConfirmedSignatureInfo;
};

export function TransactionHistoryCardWrapper({
  pubkey,
  tab,
  title,
}: {
  pubkey: PublicKey;
  tab: MoreTabs;
  title: string;
}) {
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

  const slotRows: SlotRow[] = [];

  for (var i = 0; i < transactions.length; i++) {
    const slot = transactions[i].slot;
    const slotTransactions = [transactions[i]];
    while (i + 1 < transactions.length) {
      const nextSlot = transactions[i + 1].slot;
      if (nextSlot !== slot) break;
      slotTransactions.push(transactions[++i]);
    }

    for (let slotTransaction of slotTransactions) {
      slotRows.push({
        slot,
        signature: slotTransaction.signature,
        err: slotTransaction.err,
        blockTime: slotTransaction.blockTime,
        failed: !!slotTransaction.err,
        signatureInfo: transactions[i],
      });
    }
  }

  const fetching = history.status === FetchStatus.Fetching;
  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">{title}</h3>
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

      {tab === "history" && (
        <TransactionHistoryDetails pubkey={pubkey} slotRows={slotRows} />
      )}
      {tab === "transfers" && (
        <TransfersDetails pubkey={pubkey} slotRows={slotRows} />
      )}
      {tab === "instructions" && (
        <TokenInstructionsDetails pubkey={pubkey} slotRows={slotRows} />
      )}

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
