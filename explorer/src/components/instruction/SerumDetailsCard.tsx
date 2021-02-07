import React from "react";
import { TransactionInstruction, SignatureResult } from "@safecoin/web3.js";
import { InstructionCard } from "./InstructionCard";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import {
  decodeCancelOrder,
  decodeCancelOrderByClientId,
  decodeConsumeEvents,
  decodeInitializeMarket,
  decodeMatchOrders,
  decodeNewOrder,
  decodeSettleFunds,
  parseSerumInstructionCode,
  parseSerumInstructionKey,
  parseSerumInstructionTitle,
  SERUM_DECODED_MAX,
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
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, signature, innerCards, childIndex } = props;

  const { url } = useCluster();

  let title;
  try {
    title = parseSerumInstructionTitle(ix);
    const code = parseSerumInstructionCode(ix);

    if (code <= SERUM_DECODED_MAX) {
      switch (parseSerumInstructionKey(ix)) {
        case "initializeMarket":
          return (
            <InitializeMarketDetailsCard
              info={decodeInitializeMarket(ix)}
              {...props}
            />
          );
        case "newOrder":
          return <NewOrderDetailsCard info={decodeNewOrder(ix)} {...props} />;
        case "matchOrders":
          return (
            <MatchOrdersDetailsCard info={decodeMatchOrders(ix)} {...props} />
          );
        case "consumeEvents":
          return (
            <ConsumeEventsDetailsCard
              info={decodeConsumeEvents(ix)}
              {...props}
            />
          );
        case "cancelOrder":
          return (
            <CancelOrderDetailsCard info={decodeCancelOrder(ix)} {...props} />
          );
        case "cancelOrderByClientId":
          return (
            <CancelOrderByClientIdDetailsCard
              info={decodeCancelOrderByClientId(ix)}
              {...props}
            />
          );
        case "settleFunds":
          return (
            <SettleFundsDetailsCard info={decodeSettleFunds(ix)} {...props} />
          );
      }
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
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
