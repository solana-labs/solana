import React from "react";
import { ErrorCard } from "components/common/ErrorCard";
import {
  ParsedInnerInstruction,
  ParsedInstruction,
  ParsedTransaction,
  PartiallyDecodedInstruction,
  PublicKey,
  SignatureResult,
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
import { WormholeDetailsCard } from "components/instruction/WormholeDetailsCard";
import { UnknownDetailsCard } from "components/instruction/UnknownDetailsCard";
import { BonfidaBotDetailsCard } from "components/instruction/BonfidaBotDetails";
import {
  INNER_INSTRUCTIONS_START_SLOT,
  SignatureProps,
} from "pages/TransactionDetailsPage";
import { intoTransactionInstruction } from "utils/tx";
import { isSerumInstruction } from "components/instruction/serum/types";
import { isTokenLendingInstruction } from "components/instruction/token-lending/types";
import { isTokenSwapInstruction } from "components/instruction/token-swap/types";
import { isBonfidaBotInstruction } from "components/instruction/bonfida-bot/types";
import { useFetchTransactionDetails } from "providers/transactions/parsed";
import {
  useTransactionDetails,
  useTransactionStatus,
} from "providers/transactions";
import { Cluster, useCluster } from "providers/cluster";
import { BpfUpgradeableLoaderDetailsCard } from "components/instruction/bpf-upgradeable-loader/BpfUpgradeableLoaderDetailsCard";
import { VoteDetailsCard } from "components/instruction/vote/VoteDetailsCard";
import { isWormholeInstruction } from "components/instruction/wormhole/types";
import { AssociatedTokenDetailsCard } from "components/instruction/AssociatedTokenDetailsCard";
import { MangoDetailsCard } from "components/instruction/MangoDetails";
import { isPythInstruction } from "components/instruction/pyth/types";
import { PythDetailsCard } from "components/instruction/pyth/PythDetailsCard";
import { isInstructionFromAnAnchorProgram } from "../instruction/anchor/types";
import { GenericAnchorDetailsCard } from "../instruction/GenericAnchorDetails";
import { isMangoInstruction } from "../instruction/mango/types";

export type InstructionDetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function InstructionsSection({ signature }: SignatureProps) {
  const status = useTransactionStatus(signature);
  const details = useTransactionDetails(signature);
  const { cluster } = useCluster();
  const fetchDetails = useFetchTransactionDetails();
  const refreshDetails = () => fetchDetails(signature);

  if (!status?.data?.info || !details?.data?.transaction) return null;

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
      });
    }
  );

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">
            <h3 className="mb-0">
              {instructionDetails.length > 1 ? "Instructions" : "Instruction"}
            </h3>
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
}: {
  ix: ParsedInstruction | PartiallyDecodedInstruction;
  tx: ParsedTransaction;
  result: SignatureResult;
  index: number;
  signature: TransactionSignature;
  innerCards?: JSX.Element[];
  childIndex?: number;
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
        return <BpfUpgradeableLoaderDetailsCard {...props} />;
      case "system":
        return <SystemDetailsCard {...props} />;
      case "stake":
        return <StakeDetailsCard {...props} />;
      case "spl-memo":
        return <MemoDetailsCard {...props} />;
      case "spl-associated-token-account":
        return <AssociatedTokenDetailsCard {...props} />;
      case "vote":
        return <VoteDetailsCard {...props} />;
      default:
        return <UnknownDetailsCard {...props} />;
    }
  }

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

  if (isBonfidaBotInstruction(transactionIx)) {
    return <BonfidaBotDetailsCard key={key} {...props} />;
  } else if (isInstructionFromAnAnchorProgram(transactionIx)) {
    return <GenericAnchorDetailsCard key={key} {...props} />;
  } else if (isMangoInstruction(transactionIx)) {
    return <MangoDetailsCard key={key} {...props} />;
  } else if (isSerumInstruction(transactionIx)) {
    return <SerumDetailsCard key={key} {...props} />;
  } else if (isTokenSwapInstruction(transactionIx)) {
    return <TokenSwapDetailsCard key={key} {...props} />;
  } else if (isTokenLendingInstruction(transactionIx)) {
    return <TokenLendingDetailsCard key={key} {...props} />;
  } else if (isWormholeInstruction(transactionIx)) {
    return <WormholeDetailsCard key={key} {...props} />;
  } else if (isPythInstruction(transactionIx)) {
    return <PythDetailsCard key={key} {...props} />;
  } else {
    return <UnknownDetailsCard key={key} {...props} />;
  }
}
