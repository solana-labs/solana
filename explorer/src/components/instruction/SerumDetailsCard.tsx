import React from "react";
import * as Sentry from "@sentry/react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { parseSerumInstructionTitle } from "utils/tx";
import { useCluster } from "providers/cluster";

export function SerumDetailsCard({
  ix,
  index,
  result,
  signature,
}: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
}) {
  const { url } = useCluster();

  let title;
  try {
    title = parseSerumInstructionTitle(ix);
  } catch (error) {
    Sentry.captureException(error, {
      tags: {
        url: url,
        signature: signature,
      },
    });
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={title || "Unknown"}
      defaultRaw
    />
  );
}
