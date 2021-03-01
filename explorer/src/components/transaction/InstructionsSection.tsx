import React from "react";

import { ErrorCard } from "components/common/ErrorCard";
import {
  ParsedInnerInstruction,
  ParsedInstruction,
  ParsedTransaction,
  PartiallyDecodedInstruction,
  PublicKey,
  SignatureResult,
  Transaction,
  TransactionSignature,
} from "@solana/web3.js";
import { BpfLoaderDetailsCard } from "components/instruction/bpf-loader/BpfLoaderDetailsCard";
import { MemoDetailsCard } from "components/instruction/MemoDetailsCard";
import { SerumDetailsCard } from "components/instruction/SerumDetailsCard";
import { StakeDetailsCard } from "components/instruction/stake/StakeDetailsCard";
import { SystemDetailsCard } from "components/instruction/system/SystemDetailsCard";
import { TokenDetailsCard } from "components/instruction/token/TokenDetailsCard";
import { TokenLendingDetailsCard } from "components/instruction/TokenLendingDetailsCard";
import { TokenSwapDetailsCard } from "components/instruction/TokenSwapDetailsCard";
import { UnknownDetailsCard } from "components/instruction/UnknownDetailsCard";
import {
  SignatureProps,
  INNER_INSTRUCTIONS_START_SLOT,
} from "pages/TransactionDetailsPage";
import { intoTransactionInstruction } from "utils/tx";
import { isSerumInstruction } from "components/instruction/serum/types";
import { isTokenLendingInstruction } from "components/instruction/token-lending/types";
import { isTokenSwapInstruction } from "components/instruction/token-swap/types";
import { useFetchTransactionDetails } from "providers/transactions/details";
import {
  useTransactionDetails,
  useTransactionStatus,
} from "providers/transactions";
import { Cluster, useCluster } from "providers/cluster";
// import { VoteDetailsCard } from "components/instruction/vote/VoteDetailsCard";
import { UpgradeableBpfLoaderDetailsCard } from "components/instruction/upgradeable-bpf-loader/UpgradeableBpfLoaderDetailsCard";

export function InstructionsSection({ signature }: SignatureProps) {
  const status = useTransactionStatus(signature);
  const details = useTransactionDetails(signature);
  const { cluster } = useCluster();
  const fetchDetails = useFetchTransactionDetails();
  const refreshDetails = () => fetchDetails(signature);

  if (!status?.data?.info || !details?.data?.transaction) return null;

  const raw = details.data.raw?.transaction;

  const { transaction } = details.data.transaction;
  const { meta } = details.data.transaction;

  if (transaction.message.instructions.length === 0) {
    return <ErrorCard retry={refreshDetails} text="No instructions found" />;
  }

  const innerInstructions: {
    [index: number]: (ParsedInstruction | PartiallyDecodedInstruction)[];
  } = {};

  if (
    meta?.innerInstructions &&
    (cluster !== Cluster.MainnetBeta ||
      details.data.transaction.slot >= INNER_INSTRUCTIONS_START_SLOT)
  ) {
    meta.innerInstructions.forEach((parsed: ParsedInnerInstruction) => {
      if (!innerInstructions[parsed.index]) {
        innerInstructions[parsed.index] = [];
      }

      parsed.instructions.forEach((ix) => {
        innerInstructions[parsed.index].push(ix);
      });
    });
  }

  const result = status.data.info.result;
  const instructionDetails = transaction.message.instructions.map(
    (instruction, index) => {
      let innerCards: JSX.Element[] = [];

      if (index in innerInstructions) {
        innerInstructions[index].forEach((ix, childIndex) => {
          if (typeof ix.programId === "string") {
            ix.programId = new PublicKey(ix.programId);
          }

          let res = renderInstructionCard({
            index,
            ix,
            result,
            signature,
            tx: transaction,
            childIndex,
            raw,
          });

          innerCards.push(res);
        });
      }

      return renderInstructionCard({
        index,
        ix: instruction,
        result,
        signature,
        tx: transaction,
        innerCards,
        raw,
      });
    }
  );

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">
            <h3 className="mb-0">Instruction(s)</h3>
          </div>
        </div>
      </div>
      {instructionDetails}
    </>
  );
}

function renderInstructionCard({
  ix,
  tx,
  result,
  index,
  signature,
  innerCards,
  childIndex,
  raw,
}: {
  ix: ParsedInstruction | PartiallyDecodedInstruction;
  tx: ParsedTransaction;
  result: SignatureResult;
  index: number;
  signature: TransactionSignature;
  innerCards?: JSX.Element[];
  childIndex?: number;
  raw?: Transaction;
}) {
  const key = `${index}-${childIndex}`;

  if ("parsed" in ix) {
    const props = {
      tx,
      ix,
      result,
      index,
      innerCards,
      childIndex,
      key,
    };

    switch (ix.program) {
      case "spl-token":
        return <TokenDetailsCard {...props} />;
      case "bpf-loader":
        return <BpfLoaderDetailsCard {...props} />;
      case "bpf-upgradeable-loader":
        return <UpgradeableBpfLoaderDetailsCard {...props} />;
      case "system":
        return <SystemDetailsCard {...props} />;
      case "stake":
        return <StakeDetailsCard {...props} />;
      case "spl-memo":
        return <MemoDetailsCard {...props} />;
      /*case "vote":
        return <VoteDetailsCard {...props} />;*/
      default:
        return <UnknownDetailsCard {...props} />;
    }
  }

  // TODO: There is a bug in web3, where inner instructions
  // aren't getting coerced. This is a temporary fix.

  if (typeof ix.programId === "string") {
    ix.programId = new PublicKey(ix.programId);
  }

  ix.accounts = ix.accounts.map((account) => {
    if (typeof account === "string") {
      return new PublicKey(account);
    }

    return account;
  });

  // TODO: End hotfix

  const transactionIx = intoTransactionInstruction(tx, ix);

  if (!transactionIx) {
    return (
      <ErrorCard
        key={key}
        text="Could not display this instruction, please report"
      />
    );
  }

  const props = {
    ix: transactionIx,
    result,
    index,
    signature,
    innerCards,
    childIndex,
  };

  if (isSerumInstruction(transactionIx)) {
    return <SerumDetailsCard key={key} {...props} />;
  } else if (isTokenSwapInstruction(transactionIx)) {
    return <TokenSwapDetailsCard key={key} {...props} />;
  } else if (isTokenLendingInstruction(transactionIx)) {
    return <TokenLendingDetailsCard key={key} {...props} />;
  } else {
    return <UnknownDetailsCard key={key} {...props} />;
  }
}
