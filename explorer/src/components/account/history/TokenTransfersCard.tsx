import React from "react";
import {
  ParsedConfirmedTransaction,
  ParsedInstruction,
  PartiallyDecodedInstruction,
  PublicKey,
} from "@solana/web3.js";
import { useAccountHistory } from "providers/accounts";
import { useTokenRegistry } from "providers/mints/token-registry";
import { create } from "superstruct";
import {
  TokenInstructionType,
  Transfer,
  TransferChecked,
} from "components/instruction/token/types";
import { InstructionContainer } from "utils/instruction";
import { Signature } from "components/common/Signature";
import { Address } from "components/common/Address";
import { normalizeTokenAmount } from "utils";
import {
  getTransactionRows,
  HistoryCardFooter,
  HistoryCardHeader,
} from "../HistoryCardComponents";
import { LoadingCard } from "components/common/LoadingCard";
import { useFetchAccountHistory } from "providers/accounts/history";
import { ErrorCard } from "components/common/ErrorCard";
import { FetchStatus } from "providers/cache";
import Moment from "react-moment";
import { extractMintDetails, MintDetails } from "./common";
import { Cluster, useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";

type IndexedTransfer = {
  index: number;
  childIndex?: number;
  transfer: Transfer | TransferChecked;
};

export function TokenTransfersCard({ pubkey }: { pubkey: PublicKey }) {
  const { cluster } = useCluster();
  const address = pubkey.toBase58();
  const history = useAccountHistory(address);
  const fetchAccountHistory = useFetchAccountHistory();
  const refresh = () => fetchAccountHistory(pubkey, true, true);
  const loadMore = () => fetchAccountHistory(pubkey, true);

  const { tokenRegistry } = useTokenRegistry();

  const mintDetails = React.useMemo(
    () => tokenRegistry.get(address),
    [address, tokenRegistry]
  );

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
    const mintMap = new Map<string, MintDetails>();

    transactionRows.forEach(
      ({ signature, blockTime, statusText, statusClass }) => {
        const parsed = detailedHistoryMap.get(signature);
        if (!parsed) return;

        // Extract mint information from token deltas
        // (used to filter out non-checked tokens transfers not belonging to this mint)
        extractMintDetails(parsed, mintMap);

        // Extract all transfers from transaction
        let transfers: IndexedTransfer[] = [];
        InstructionContainer.create(parsed).instructions.forEach(
          ({ instruction, inner }, index) => {
            const transfer = getTransfer(instruction, cluster, signature);
            if (transfer) {
              transfers.push({
                transfer,
                index,
              });
            }
            inner.forEach((instruction, childIndex) => {
              const transfer = getTransfer(instruction, cluster, signature);
              if (transfer) {
                transfers.push({
                  transfer,
                  index,
                  childIndex,
                });
              }
            });
          }
        );

        // Filter out transfers not belonging to this mint
        transfers = transfers.filter(({ transfer }) => {
          const sourceKey = transfer.source.toBase58();
          const destinationKey = transfer.destination.toBase58();

          if ("tokenAmount" in transfer && transfer.mint.equals(pubkey)) {
            return true;
          } else if (
            mintMap.has(sourceKey) &&
            mintMap.get(sourceKey)?.mint === address
          ) {
            return true;
          } else if (
            mintMap.has(destinationKey) &&
            mintMap.get(destinationKey)?.mint === address
          ) {
            return true;
          }

          return false;
        });

        transfers.forEach(({ transfer, index, childIndex }) => {
          let units = "Tokens";
          let amountString = "";

          if (mintDetails?.symbol) {
            units = mintDetails.symbol;
          }

          if ("tokenAmount" in transfer) {
            amountString = transfer.tokenAmount.uiAmountString;
          } else {
            let decimals = 0;

            if (mintDetails?.decimals) {
              decimals = mintDetails.decimals;
            } else if (mintMap.has(transfer.source.toBase58())) {
              decimals = mintMap.get(transfer.source.toBase58())?.decimals || 0;
            } else if (mintMap.has(transfer.destination.toBase58())) {
              decimals =
                mintMap.get(transfer.destination.toBase58())?.decimals || 0;
            }

            amountString = new Intl.NumberFormat("en-US", {
              minimumFractionDigits: decimals,
              maximumFractionDigits: decimals,
            }).format(normalizeTokenAmount(transfer.amount, decimals));
          }

          detailsList.push(
            <tr key={signature + index + (childIndex || "")}>
              <td>
                <Signature signature={signature} link truncateChars={24} />
              </td>

              {hasTimestamps && (
                <td className="text-muted">
                  {blockTime && <Moment date={blockTime * 1000} fromNow />}
                </td>
              )}

              <td>
                <Address pubkey={transfer.source} link truncateChars={16} />
              </td>

              <td>
                <Address
                  pubkey={transfer.destination}
                  link
                  truncateChars={16}
                />
              </td>

              <td>
                {amountString} {units}
              </td>

              <td>
                <span className={`badge bg-${statusClass}-soft`}>
                  {statusText}
                </span>
              </td>
            </tr>
          );
        });
      }
    );

    return {
      hasTimestamps,
      detailsList,
    };
  }, [history, transactionRows, mintDetails, pubkey, address, cluster]);

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
        title="Token Transfers"
      />
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Transaction Signature</th>
              {hasTimestamps && <th className="text-muted">Age</th>}
              <th className="text-muted">Source</th>
              <th className="text-muted">Destination</th>
              <th className="text-muted">Amount</th>
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

function getTransfer(
  instruction: ParsedInstruction | PartiallyDecodedInstruction,
  cluster: Cluster,
  signature: string
): Transfer | TransferChecked | undefined {
  if ("parsed" in instruction && instruction.program === "spl-token") {
    try {
      const { type: rawType } = instruction.parsed;
      const type = create(rawType, TokenInstructionType);

      if (type === "transferChecked") {
        return create(instruction.parsed.info, TransferChecked);
      } else if (type === "transfer") {
        return create(instruction.parsed.info, Transfer);
      }
    } catch (error) {
      if (cluster === Cluster.MainnetBeta) {
        reportError(error, {
          signature,
        });
      }
    }
  }
  return undefined;
}
