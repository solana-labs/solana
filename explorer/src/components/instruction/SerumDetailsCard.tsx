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
        return <InitializeMarketDetailsCard info={BuildInitializeMarket(ix)} {...props} />
      case "newOrder":
        return <NewOrderDetailsCard info={BuildNewOrder(ix)} {...props}/>
      case "matchOrders":
        return <MatchOrdersDetailsCard info={BuildMatchOrders(ix)} {...props} />
      case "consumeEvents":
        return <ConsumeEventsDetailsCard info={BuildConsumeEvents(ix)} {...props} />
      case "cancelOrder":
        return <CancelOrderDetailsCard info={BuildCancelOrder(ix)} {...props} />
      case "cancelOrderByClientId":
        return <CancelOrderByClientIdDetailsCard info={BuildCancelOrderByClientId(ix)} {...props} />
      case "settleFunds":
        return <SettleFundsDetailsCard info={BuildSettleFunds(ix)} {...props} />
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
