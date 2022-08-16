import React from "react";
import { ErrorCard } from "components/common/ErrorCard";
import {
  ComputeBudgetProgram,
  ParsedInnerInstruction,
  ParsedInstruction,
  ParsedTransaction,
  PartiallyDecodedInstruction,
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
import { isAddressLookupTableInstruction } from "components/instruction/address-lookup-table/types";
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
import AnchorDetailsCard from "../instruction/AnchorDetailsCard";
import { isMangoInstruction } from "../instruction/mango/types";
import { useAnchorProgram } from "providers/anchor";
import { LoadingCard } from "components/common/LoadingCard";
import { ErrorBoundary } from "@sentry/react";
import { ComputeBudgetDetailsCard } from "components/instruction/ComputeBudgetDetailsCard";
import { AddressLookupTableDetailsCard } from "components/instruction/AddressLookupTableDetailsCard";

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
  const { cluster, url } = useCluster();
  const fetchDetails = useFetchTransactionDetails();
  const refreshDetails = () => fetchDetails(signature);

  const result = status?.data?.info?.result;
  const transactionWithMeta = details?.data?.transactionWithMeta;
  if (!result || !transactionWithMeta) {
    return <ErrorCard retry={refreshDetails} text="No instructions found" />;
  }
  const { meta, transaction } = transactionWithMeta;

  if (transaction.message.instructions.length === 0) {
    return <ErrorCard retry={refreshDetails} text="No instructions found" />;
  }

  const innerInstructions: {
    [index: number]: (ParsedInstruction | PartiallyDecodedInstruction)[];
  } = {};

  if (
    meta?.innerInstructions &&
    (cluster !== Cluster.MainnetBeta ||
      transactionWithMeta.slot >= INNER_INSTRUCTIONS_START_SLOT)
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

  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">
            <h3 className="mb-0">
              {transaction.message.instructions.length > 1
                ? "Instructions"
                : "Instruction"}
            </h3>
          </div>
        </div>
      </div>
      <React.Suspense fallback={<LoadingCard message="Loading Instructions" />}>
        {transaction.message.instructions.map((instruction, index) => {
          let innerCards: JSX.Element[] = [];

          if (index in innerInstructions) {
            innerInstructions[index].forEach((ix, childIndex) => {
              let res = (
                <InstructionCard
                  key={`${index}-${childIndex}`}
                  index={index}
                  ix={ix}
                  result={result}
                  signature={signature}
                  tx={transaction}
                  childIndex={childIndex}
                  url={url}
                />
              );
              innerCards.push(res);
            });
          }

          return (
            <InstructionCard
              key={`${index}`}
              index={index}
              ix={instruction}
              result={result}
              signature={signature}
              tx={transaction}
              innerCards={innerCards}
              url={url}
            />
          );
        })}
      </React.Suspense>
    </>
  );
}

function InstructionCard({
  ix,
  tx,
  result,
  index,
  signature,
  innerCards,
  childIndex,
  url,
}: {
  ix: ParsedInstruction | PartiallyDecodedInstruction;
  tx: ParsedTransaction;
  result: SignatureResult;
  index: number;
  signature: TransactionSignature;
  innerCards?: JSX.Element[];
  childIndex?: number;
  url: string;
}) {
  const key = `${index}-${childIndex}`;
  const anchorProgram = useAnchorProgram(ix.programId.toString(), url);

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

  if (isAddressLookupTableInstruction(transactionIx)) {
    return <AddressLookupTableDetailsCard key={key} {...props} />;
  } else if (isBonfidaBotInstruction(transactionIx)) {
    return <BonfidaBotDetailsCard key={key} {...props} />;
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
  } else if (ComputeBudgetProgram.programId.equals(transactionIx.programId)) {
    return <ComputeBudgetDetailsCard key={key} {...props} />;
  } else if (anchorProgram) {
    return (
      <ErrorBoundary fallback={<UnknownDetailsCard {...props} />}>
        <AnchorDetailsCard key={key} anchorProgram={anchorProgram} {...props} />
      </ErrorBoundary>
    );
  } else {
    return <UnknownDetailsCard key={key} {...props} />;
  }
}
