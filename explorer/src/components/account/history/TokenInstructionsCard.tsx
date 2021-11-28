import React from "react";
import {
  ParsedConfirmedTransaction,
  ParsedInstruction,
  PartiallyDecodedInstruction,
  PublicKey,
} from "@solana/web3.js";
import { useAccountHistory } from "providers/accounts";
import { Signature } from "components/common/Signature";
import {
  getTokenInstructionName,
  InstructionContainer,
} from "utils/instruction";
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
import { extractMintDetails, MintDetails } from "./common";
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
    const mintMap = new Map<string, MintDetails>();

    transactionRows.forEach(
      ({ signatureInfo, signature, blockTime, statusClass, statusText }) => {
        const parsed = detailedHistoryMap.get(signature);
        if (!parsed) return;

        extractMintDetails(parsed, mintMap);

        let instructions: (ParsedInstruction | PartiallyDecodedInstruction)[] =
          [];

        InstructionContainer.create(parsed).instructions.forEach(
          ({ instruction, inner }, index) => {
            if (isRelevantInstruction(pubkey, address, mintMap, instruction)) {
              instructions.push(instruction);
            }
            instructions.push(
              ...inner.filter((instruction) =>
                isRelevantInstruction(pubkey, address, mintMap, instruction)
              )
            );
          }
        );

        instructions.forEach((ix, index) => {
          const programId = ix.programId;

          const instructionName = getTokenInstructionName(
            parsed,
            ix,
            signatureInfo
          );

          if (instructionName) {
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

                <td>{instructionName}</td>

                <td>
                  <Address
                    pubkey={programId}
                    link
                    truncate
                    truncateChars={16}
                  />
                </td>

                <td>
                  <span className={`badge bg-${statusClass}-soft`}>
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
  }, [history, transactionRows, address, pubkey]);

  if (!history) {
    return null;
  }

  if (history?.data === undefined) {
    if (history.status === FetchStatus.Fetching) {
      return <LoadingCard message="Loading token instructions" />;
    }

    return (
      <ErrorCard retry={refresh} text="Failed to fetch token instructions" />
    );
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

function isRelevantInstruction(
  pubkey: PublicKey,
  address: string,
  mintMap: Map<string, MintDetails>,
  instruction: ParsedInstruction | PartiallyDecodedInstruction
) {
  if ("accounts" in instruction) {
    return instruction.accounts.some(
      (account) =>
        account.equals(pubkey) ||
        mintMap.get(account.toBase58())?.mint === address
    );
  } else if (
    typeof instruction.parsed === "object" &&
    "info" in instruction.parsed
  ) {
    return Object.entries(instruction.parsed.info).some(
      ([key, value]) =>
        value === address ||
        (typeof value === "string" && mintMap.get(value)?.mint === address)
    );
  }
  return false;
}
