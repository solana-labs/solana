import React from "react";
import { TransactionInstruction, SignatureResult } from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import {
  decodeCancelOrder,
  decodeInitializeBot,
  decodeCreateBot,
  decodeDeposit,
  decodeCreateOrder,
  decodeSettleFunds,
  decodeRedeem,
  decodeCollectFees,
  parseBonfidaBotInstructionTitle,
} from "./bonfida-bot/types";
import { CancelOrderDetailsCard } from "./bonfida-bot/CancelOrderDetails";
import { CollectFeesDetailsCard } from "./bonfida-bot/CollectFeesDetails";
import { CreateBotDetailsCard } from "./bonfida-bot/CreateBotDetails";
import { DepositDetailsCard } from "./bonfida-bot/DepositDetails";
import { InitializeBotDetailsCard } from "./bonfida-bot/InitializeBotDetails";
import { RedeemDetailsCard } from "./bonfida-bot/RedeemDetails";
import { SettleFundsDetailsCard } from "./bonfida-bot/SettleFundsDetails";
import { CreateOrderDetailsCard } from "./bonfida-bot/CreateOrderDetails";

export function BonfidaBotDetailsCard(props: {
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
    title = parseBonfidaBotInstructionTitle(ix);

    switch (title) {
      case "Initialize Bot":
        return (
          <InitializeBotDetailsCard info={decodeInitializeBot(ix)} {...props} />
        );
      case "Create Bot":
        return <CreateBotDetailsCard info={decodeCreateBot(ix)} {...props} />;
      case "Deposit":
        return <DepositDetailsCard info={decodeDeposit(ix)} {...props} />;
      case "Create Order":
        return (
          <CreateOrderDetailsCard info={decodeCreateOrder(ix)} {...props} />
        );
      case "Cancel Order":
        return (
          <CancelOrderDetailsCard info={decodeCancelOrder(ix)} {...props} />
        );
      case "Settle Funds":
        return (
          <SettleFundsDetailsCard info={decodeSettleFunds(ix)} {...props} />
        );
      case "settleFunds":
        return (
          <SettleFundsDetailsCard info={decodeSettleFunds(ix)} {...props} />
        );
      case "Redeem":
        return <RedeemDetailsCard info={decodeRedeem(ix)} {...props} />;
      case "Collect Fees":
        return (
          <CollectFeesDetailsCard info={decodeCollectFees(ix)} {...props} />
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
      title={`Bonfida Bot: ${title || "Unknown"}`}
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
