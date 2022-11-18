import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import {
  decodeCancelOrder,
  decodeCancelOrderByClientId,
  decodeCancelOrderByClientIdV2,
  decodeCancelOrderV2,
  decodeCloseOpenOrders,
  decodeConsumeEvents,
  decodeConsumeEventsPermissioned,
  decodeDisableMarket,
  decodeInitializeMarket,
  decodeInitOpenOrders,
  decodeMatchOrders,
  decodeNewOrder,
  decodeNewOrderV3,
  decodePrune,
  decodeSettleFunds,
  decodeSweepFees,
  OPEN_BOOK_PROGRAM_ID,
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
import { DisableMarketDetailsCard } from "./serum/DisableMarketDetails";
import { SweepFeesDetailsCard } from "./serum/SweepFeesDetails";
import { NewOrderV3DetailsCard } from "./serum/NewOrderV3DetailsCard";
import { CancelOrderV2DetailsCard } from "./serum/CancelOrderV2Details";
import { CancelOrderByClientIdV2DetailsCard } from "./serum/CancelOrderByClientIdV2Details";
import { CloseOpenOrdersDetailsCard } from "./serum/CloseOpenOrdersDetails";
import { InitOpenOrdersDetailsCard } from "./serum/InitOpenOrdersDetails";
import { PruneDetailsCard } from "./serum/PruneDetails";
import { ConsumeEventsPermissionedDetailsCard } from "./serum/ConsumeEventsPermissionedDetails";

export function SerumDetailsCard(initialProps: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, signature, innerCards, childIndex } = initialProps;

  const props = React.useMemo(() => {
    const programName =
      initialProps.ix.programId.toBase58() === OPEN_BOOK_PROGRAM_ID
        ? "OpenBook"
        : "Serum";
    return { ...initialProps, programName };
  }, [initialProps]);

  const { url } = useCluster();

  let title;
  try {
    title = parseSerumInstructionTitle(ix);
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
          <ConsumeEventsDetailsCard info={decodeConsumeEvents(ix)} {...props} />
        );
      case "cancelOrder":
        return (
          <CancelOrderDetailsCard info={decodeCancelOrder(ix)} {...props} />
        );
      case "settleFunds":
        return (
          <SettleFundsDetailsCard info={decodeSettleFunds(ix)} {...props} />
        );
      case "cancelOrderByClientId":
        return (
          <CancelOrderByClientIdDetailsCard
            info={decodeCancelOrderByClientId(ix)}
            {...props}
          />
        );
      case "disableMarket":
        return (
          <DisableMarketDetailsCard info={decodeDisableMarket(ix)} {...props} />
        );
      case "sweepFees":
        return <SweepFeesDetailsCard info={decodeSweepFees(ix)} {...props} />;
      case "newOrderV3":
        return <NewOrderV3DetailsCard info={decodeNewOrderV3(ix)} {...props} />;
      case "cancelOrderV2":
        return (
          <CancelOrderV2DetailsCard info={decodeCancelOrderV2(ix)} {...props} />
        );
      case "cancelOrderByClientIdV2":
        return (
          <CancelOrderByClientIdV2DetailsCard
            info={decodeCancelOrderByClientIdV2(ix)}
            {...props}
          />
        );
      case "closeOpenOrders":
        return (
          <CloseOpenOrdersDetailsCard
            info={decodeCloseOpenOrders(ix)}
            {...props}
          />
        );
      case "initOpenOrders":
        return (
          <InitOpenOrdersDetailsCard
            info={decodeInitOpenOrders(ix)}
            {...props}
          />
        );
      case "prune":
        return <PruneDetailsCard info={decodePrune(ix)} {...props} />;
      case "consumeEventsPermissioned":
        return (
          <ConsumeEventsPermissionedDetailsCard
            info={decodeConsumeEventsPermissioned(ix)}
            {...props}
          />
        );
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
      title={`${props.programName} Program: ${title || "Unknown"}`}
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
