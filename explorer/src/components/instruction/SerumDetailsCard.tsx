import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import {
  BuildCancelOrder,
  BuildCancelOrderByClientId,
  BuildConsumeEvents,
  BuildInitializeMarket,
  BuildMatchOrders,
  BuildNewOrder,
  BuildSettleFunds,
  parseSerumInstructionKey,
  parseSerumInstructionTitle,
} from "./serum/types";

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

    const key = parseSerumInstructionKey(ix);

    switch (key) {
      case "initializeMarket":
        const initializeMarket = BuildInitializeMarket(ix);
        break;
      case "newOrder":
        const newOrder = BuildNewOrder(ix);
        break;
      case "matchOrders":
        const matchOrders = BuildMatchOrders(ix);
        break;
      case "consumeEvents":
        const consumeEvents = BuildConsumeEvents(ix);
        break;
      case "cancelOrder":
        const cancelOrder = BuildCancelOrder(ix);
        break;
      case "cancelOrderByClientId":
        const cancelOrderByClientId = BuildCancelOrderByClientId(ix);
        break;
      case "settleFunds":
        const settleFunds = BuildSettleFunds(ix);
        break;
    }

    console.log(key);
  } catch (error) {
    reportError(error, {
      url: url,
      signature: signature,
    });
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`Serum: ${title || "Unknown"}`}
      defaultRaw
    />
  );
}
