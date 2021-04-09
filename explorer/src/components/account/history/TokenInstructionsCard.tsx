import React from "react";
import { ParsedConfirmedTransaction, PublicKey } from "@solana/web3.js";
import { useAccountHistory } from "providers/accounts";
import { Signature } from "components/common/Signature";
import { getTokenInstructionType } from "utils/instruction";
import { InstructionDetails } from "components/common/InstructionDetails";
import { Address } from "components/common/Address";
import { LoadingCard } from "components/common/LoadingCard";
import { ErrorCard } from "components/common/ErrorCard";
import { FetchStatus } from "providers/cache";
import { useFetchAccountHistory } from "providers/accounts/history";
import {
  getTransactionRows,
  HistoryCardFooter,
  HistoryCardHeader,
} from "../HistoryCardComponents";
import Moment from "react-moment";

export function TokenInstructionsCard({ pubkey }: { pubkey: PublicKey }) {
  const address = pubkey.toBase58();
  const history = useAccountHistory(address);
  const fetchAccountHistory = useFetchAccountHistory();
  const refresh = () => fetchAccountHistory(pubkey, true, true);
  const loadMore = () => fetchAccountHistory(pubkey, true);

  const transactionRows = React.useMemo(() => {
    if (history?.data?.fetched) {
      return getTransactionRows(history.data.fetched);
    }

    return [];
  }, [history]);

  React.useEffect(() => {
    if (!history || !history.data?.transactionMap?.size) {
      refresh();
    }
  }, [address]); // eslint-disable-line react-hooks/exhaustive-deps

  const { hasTimestamps, detailsList } = React.useMemo(() => {
    const detailedHistoryMap =
      history?.data?.transactionMap ||
      new Map<string, ParsedConfirmedTransaction>();
    const hasTimestamps = transactionRows.some((element) => element.blockTime);
    const detailsList: React.ReactNode[] = [];

    transactionRows.forEach(
      ({ signatureInfo, signature, blockTime, statusClass, statusText }) => {
        const parsed = detailedHistoryMap.get(signature);
        if (!parsed) return;

        const instructions = parsed.transaction.message.instructions;

        instructions.forEach((ix, index) => {
          const instructionType = getTokenInstructionType(
            parsed,
            ix,
            signatureInfo,
            index
          );

          const programId = ix.programId;

          if (instructionType) {
            detailsList.push(
              <tr key={signature + index}>
                <td>
                  <Signature signature={signature} link truncateChars={48} />
                </td>

                {hasTimestamps && (
                  <td className="text-muted">
                    {blockTime && <Moment date={blockTime * 1000} fromNow />}
                  </td>
                )}

                <td>
                  <InstructionDetails
                    instructionType={instructionType}
                    tx={signatureInfo}
                  />
                </td>

                <td>
                  <Address
                    pubkey={programId}
                    link
                    truncate
                    truncateChars={16}
                  />
                </td>

                <td>
                  <span className={`badge badge-soft-${statusClass}`}>
                    {statusText}
                  </span>
                </td>
              </tr>
            );
          }
        });
      }
    );

    return {
      hasTimestamps,
      detailsList,
    };
  }, [history, transactionRows]);

  if (!history) {
    return null;
  }

  if (history?.data === undefined) {
    if (history.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading token transfers" />;
    }

    return <ErrorCard retry={refresh} text="Failed to fetch token transfers" />;
  }

  const fetching = history.status === FetchStatus.Fetching;
  return (
    <div className="card">
      <HistoryCardHeader
        fetching={fetching}
        refresh={() => refresh()}
        title="Token Instructions"
      />
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted w-1">Transaction Signature</th>
              {hasTimestamps && <th className="text-muted">Age</th>}
              <th className="text-muted">Instruction</th>
              <th className="text-muted">Program</th>
              <th className="text-muted">Result</th>
            </tr>
          </thead>
          <tbody className="list">{detailsList}</tbody>
        </table>
      </div>
      <HistoryCardFooter
        fetching={fetching}
        foundOldest={history.data.foundOldest}
        loadMore={() => loadMore()}
      />
    </div>
  );
}
