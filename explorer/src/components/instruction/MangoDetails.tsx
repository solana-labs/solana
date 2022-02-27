import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import { InstructionCard } from "./InstructionCard";
import { AddOracleDetailsCard } from "./mango/AddOracleDetailsCard";
import { AddPerpMarketDetailsCard } from "./mango/AddPerpMarketDetailsCard";
import { AddSpotMarketDetailsCard } from "./mango/AddSpotMarketDetailsCard";
import { CancelPerpOrderDetailsCard } from "./mango/CancelPerpOrderDetailsCard";
import { CancelSpotOrderDetailsCard } from "./mango/CancelSpotOrderDetailsCard";
import { ChangePerpMarketParamsDetailsCard } from "./mango/ChangePerpMarketParamsDetailsCard";
import { ConsumeEventsDetailsCard } from "./mango/ConsumeEventsDetailsCard";
import { GenericMngoAccountDetailsCard } from "./mango/GenericMngoAccountDetailsCard";
import { GenericPerpMngoDetailsCard } from "./mango/GenericPerpMngoDetailsCard";
import { GenericSpotMngoDetailsCard } from "./mango/GenericSpotMngoDetailsCard";
import { PlacePerpOrderDetailsCard } from "./mango/PlacePerpOrderDetailsCard";
import { PlaceSpotOrderDetailsCard } from "./mango/PlaceSpotOrderDetailsCard";
import {
  decodeAddPerpMarket,
  decodeAddSpotMarket,
  decodeCancelPerpOrder,
  decodeCancelSpotOrder,
  decodeChangePerpMarketParams,
  decodePlacePerpOrder,
  decodePlacePerpOrder2,
  decodePlaceSpotOrder,
  parseMangoInstructionTitle,
} from "./mango/types";
import { PlacePerpOrder2DetailsCard } from "./mango/PlacePerpOrder2DetailsCard";

export function MangoDetailsCard(props: {
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
    title = parseMangoInstructionTitle(ix);

    switch (title) {
      case "InitMangoAccount":
        return (
          <GenericMngoAccountDetailsCard
            mangoAccountKeyLocation={1}
            title={title}
            {...props}
          />
        );
      case "Deposit":
        return (
          <GenericMngoAccountDetailsCard
            mangoAccountKeyLocation={1}
            title={title}
            {...props}
          />
        );
      case "Withdraw":
        return (
          <GenericMngoAccountDetailsCard
            mangoAccountKeyLocation={1}
            title={title}
            {...props}
          />
        );
      case "InitSpotOpenOrders":
        return (
          <GenericMngoAccountDetailsCard
            mangoAccountKeyLocation={1}
            title={title}
            {...props}
          />
        );
      case "PlaceSpotOrder":
        return (
          <PlaceSpotOrderDetailsCard
            info={decodePlaceSpotOrder(ix)}
            {...props}
          />
        );
      case "CancelSpotOrder":
        return (
          <CancelSpotOrderDetailsCard
            info={decodeCancelSpotOrder(ix)}
            {...props}
          />
        );
      case "AddPerpMarket":
        return (
          <AddPerpMarketDetailsCard info={decodeAddPerpMarket(ix)} {...props} />
        );
      case "PlacePerpOrder":
        return (
          <PlacePerpOrderDetailsCard
            info={decodePlacePerpOrder(ix)}
            {...props}
          />
        );
      case "PlacePerpOrder2":
        return (
          <PlacePerpOrder2DetailsCard
            info={decodePlacePerpOrder2(ix)}
            {...props}
          />
        );
      case "ConsumeEvents":
        return <ConsumeEventsDetailsCard {...props} />;
      case "CancelPerpOrder":
        return (
          <CancelPerpOrderDetailsCard
            info={decodeCancelPerpOrder(ix)}
            {...props}
          />
        );
      case "SettleFunds":
        return (
          <GenericSpotMngoDetailsCard
            accountKeyLocation={2}
            spotMarketkeyLocation={5}
            title={title}
            {...props}
          />
        );
      case "RedeemMngo":
        return (
          <GenericPerpMngoDetailsCard
            mangoAccountKeyLocation={3}
            perpMarketKeyLocation={4}
            title={title}
            {...props}
          />
        );
      case "ChangePerpMarketParams":
        return (
          <ChangePerpMarketParamsDetailsCard
            info={decodeChangePerpMarketParams(ix)}
            {...props}
          />
        );
      case "AddOracle":
        return <AddOracleDetailsCard {...props} />;
      case "AddSpotMarket":
        return (
          <AddSpotMarketDetailsCard info={decodeAddSpotMarket(ix)} {...props} />
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
      title={`Mango Program: ${title || "Unknown"}`}
      innerCards={innerCards}
      childIndex={childIndex}
      defaultRaw
    />
  );
}
