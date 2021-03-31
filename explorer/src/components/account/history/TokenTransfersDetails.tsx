import React from "react";
import {
  ParsedInstruction,
  PartiallyDecodedInstruction,
  PublicKey,
} from "@solana/web3.js";
import { useAccountHistory } from "providers/accounts";
import {
  useDetailedAccountHistory,
  useFetchDetailedAccountHistory,
} from "providers/accounts/detailed-history";
import { useTokenRegistry } from "providers/mints/token-registry";
import { SlotRow } from "../TransactionHistoryCardWrapper";
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
import Moment from "react-moment";

type MintDetails = {
  decimals: number;
  mint: string;
};

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
  const mintDetails = tokenRegistry.get(pubkey.toBase58());

  React.useEffect(() => {
    if (history?.data?.fetched) {
      fetchDetailedAccountHistory(history.data.fetched);
    }
  }, [history]); // eslint-disable-line react-hooks/exhaustive-deps

  const hasTimestamps = !!slotRows.find((element) => !!element.blockTime);
  const detailsList: React.ReactNode[] = [];
  const mintDecimalsMap = new Map<string, MintDetails>();

  slotRows.forEach(({ slot, signature, signatureInfo, blockTime, failed }) => {
    const parsed = detailedHistoryMap.get(signature);
    if (!parsed) return;

    // Extract mint decimals from token deltas
    // if all else fails for old Transfer ix types
    if (parsed.meta?.preTokenBalances) {
      parsed.meta.preTokenBalances.forEach((balance) => {
        const account =
          parsed.transaction.message.accountKeys[balance.accountIndex];
        mintDecimalsMap.set(account.pubkey.toBase58(), {
          decimals: balance.uiTokenAmount.decimals,
          mint: balance.mint,
        });
      });
    }

    if (parsed.meta?.postTokenBalances) {
      parsed.meta.postTokenBalances.forEach((balance) => {
        const account =
          parsed.transaction.message.accountKeys[balance.accountIndex];
        mintDecimalsMap.set(account.pubkey.toBase58(), {
          decimals: balance.uiTokenAmount.decimals,
          mint: balance.mint,
        });
      });
    }

    // Extract all transfers from transaction
    let transfers: (Transfer | TransferChecked)[] = [];
    InstructionContainer.create(parsed).instructions.forEach(
      ({ instruction, inner }) => {
        const transfer = getTransfer(instruction);
        if (transfer) {
          transfers.push(transfer);
        }
        inner.forEach((instruction) => {
          const transfer = getTransfer(instruction);
          if (transfer) {
            transfers.push(transfer);
          }
        });
      }
    );

    // Filter out transfers not belonging to this mint
    transfers = transfers.filter((transfer) => {
      if ("tokenAmount" in transfer && transfer.mint !== pubkey) {
        return false;
      } else if (
        mintDecimalsMap.has(transfer.source.toBase58()) &&
        mintDecimalsMap.get(transfer.source.toBase58())?.mint !==
          pubkey.toBase58()
      ) {
        return false;
      } else if (
        mintDecimalsMap.has(transfer.destination.toBase58()) &&
        mintDecimalsMap.get(transfer.destination.toBase58())?.mint !==
          pubkey.toBase58()
      ) {
        return false;
      }
      return true;
    });

    transfers.forEach((transfer) => {
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
        } else if (mintDecimalsMap.has(transfer.source.toBase58())) {
          decimals =
            mintDecimalsMap.get(transfer.source.toBase58())?.decimals || 0;
        } else if (mintDecimalsMap.has(transfer.destination.toBase58())) {
          decimals =
            mintDecimalsMap.get(transfer.destination.toBase58())?.decimals || 0;
        }

        amountString = new Intl.NumberFormat("en-US", {
          minimumFractionDigits: decimals,
          maximumFractionDigits: decimals,
        }).format(normalizeTokenAmount(transfer.amount, decimals));
      }

      detailsList.push(
        <tr
          key={signature + transfer.source + transfer.destination}
          className={`${failed && "transaction-failed"}`}
          title={`${failed && "Transaction Failed"}`}
        >
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
            <Address pubkey={transfer.destination} link truncateChars={16} />
          </td>

          <td>
            {amountString} {units}
          </td>
        </tr>
      );
    });
  });

  return (
    <div className="table-responsive mb-0">
      <table className="table table-sm table-nowrap card-table">
        <thead>
          <tr>
            <th className="text-muted">Transaction Signature</th>
            {hasTimestamps && <th className="text-muted">Age</th>}
            <th className="text-muted">Source</th>
            <th className="text-muted">Destination</th>
            <th className="text-muted">Amount</th>
          </tr>
        </thead>
        <tbody className="list">{detailsList}</tbody>
      </table>
    </div>
  );
}

function getTransfer(
  instruction: ParsedInstruction | PartiallyDecodedInstruction
): Transfer | TransferChecked | undefined {
  if ("parsed" in instruction && instruction.program === "spl-token") {
    const { type: rawType } = instruction.parsed;
    const type = create(rawType, TokenInstructionType);

    if (type === "transferChecked") {
      return create(instruction.parsed.info, TransferChecked);
    } else if (type === "transfer") {
      return create(instruction.parsed.info, Transfer);
    }
  }
  return undefined;
}
