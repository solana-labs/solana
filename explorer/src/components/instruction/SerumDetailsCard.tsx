import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import {
  buildCancelOrder,
  buildCancelOrderByClientId,
  buildConsumeEvents,
  buildInitializeMarket,
  buildMatchOrders,
  buildNewOrder,
  buildSettleFunds,
  parseSerumInstructionKey,
  parseSerumInstructionTitle,
} from "./serum/types";
import { NewOrderDetailsCard } from "./serum/NewOrderDetailsCard";
import { MatchOrdersDetailsCard } from "./serum/MatchOrdersDetailsCard";
import { InitializeMarketDetailsCard } from "./serum/InitializeMarketDetailsCard";
import { ConsumeEventsDetailsCard } from "./serum/ConsumeEventsDetails";
import { CancelOrderDetailsCard } from "./serum/CancelOrderDetails";
import { CancelOrderByClientIdDetailsCard } from "./serum/CancelOrderByClientIdDetails";
import { SettleFundsDetailsCard } from "./serum/SettleFundsDetailsCard";

export function SerumDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
}) {
  const {
    ix,
    index,
    result,
    signature,
  } = props;

  const { url } = useCluster();

  let title;
  try {
    title = parseSerumInstructionTitle(ix);

    switch (parseSerumInstructionKey(ix)) {
      case "initializeMarket":
        return <InitializeMarketDetailsCard info={buildInitializeMarket(ix)} {...props} />
      case "newOrder":
        return <NewOrderDetailsCard info={buildNewOrder(ix)} {...props}/>
      case "matchOrders":
        return <MatchOrdersDetailsCard info={buildMatchOrders(ix)} {...props} />
      case "consumeEvents":
        return <ConsumeEventsDetailsCard info={buildConsumeEvents(ix)} {...props} />
      case "cancelOrder":
        return <CancelOrderDetailsCard info={buildCancelOrder(ix)} {...props} />
      case "cancelOrderByClientId":
        return <CancelOrderByClientIdDetailsCard info={buildCancelOrderByClientId(ix)} {...props} />
      case "settleFunds":
        return <SettleFundsDetailsCard info={buildSettleFunds(ix)} {...props} />
    }
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
