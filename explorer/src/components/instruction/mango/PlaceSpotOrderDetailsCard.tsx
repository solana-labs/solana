import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import BN from "bn.js";
import { Address } from "components/common/Address";
import { useCluster } from "providers/cluster";
import { useEffect, useState } from "react";
import { InstructionCard } from "../InstructionCard";
import {
  getSpotMarketFromInstruction,
  getSpotMarketFromSpotMarketConfig,
  OrderLotDetails,
  PlaceSpotOrder,
} from "./types";

export function PlaceSpotOrderDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: PlaceSpotOrder;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;
  const mangoAccount = ix.keys[1];
  const spotMarketAccountMeta = ix.keys[5];
  const mangoSpotMarketConfig = getSpotMarketFromInstruction(
    ix,
    spotMarketAccountMeta
  );

  const cluster = useCluster();
  const [orderLotDetails, setOrderLotDetails] =
    useState<OrderLotDetails | null>(null);
  useEffect(() => {
    async function getOrderLotDetails() {
      if (mangoSpotMarketConfig === undefined) {
        return;
      }
      const mangoSpotMarket = await getSpotMarketFromSpotMarketConfig(
        ix.programId,
        cluster.url,
        mangoSpotMarketConfig
      );
      if (mangoSpotMarket === undefined) {
        return;
      }
      const maxBaseQuantity = mangoSpotMarket.baseSizeLotsToNumber(
        new BN(info.maxBaseQuantity.toString())
      );
      const limitPrice = mangoSpotMarket.priceLotsToNumber(
        new BN(info.limitPrice.toString())
      );
      setOrderLotDetails({
        price: limitPrice,
        size: maxBaseQuantity,
      } as OrderLotDetails);
    }
    getOrderLotDetails();
  }, [
    cluster.url,
    info.maxBaseQuantity,
    info.limitPrice,
    ix.programId,
    mangoSpotMarketConfig,
  ]);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango Program: PlaceSpotOrder"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Mango account</td>
        <td>
          {" "}
          <Address pubkey={mangoAccount.pubkey} alignRight link />
        </td>
      </tr>

      {mangoSpotMarketConfig !== undefined && (
        <tr>
          <td>Spot market</td>
          <td className="text-lg-end">{mangoSpotMarketConfig.name}</td>
        </tr>
      )}

      <tr>
        <td>Spot market address</td>
        <td>
          <Address pubkey={spotMarketAccountMeta.pubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Order type</td>
        <td className="text-lg-end">{info.orderType}</td>
      </tr>

      {info.clientId !== "0" && (
        <tr>
          <td>Client Id</td>
          <td className="text-lg-end">{info.clientId}</td>
        </tr>
      )}

      <tr>
        <td>Side</td>
        <td className="text-lg-end">{info.side}</td>
      </tr>

      {orderLotDetails !== null && (
        <tr>
          <td>Limit price</td>
          {/* todo fix price */}
          <td className="text-lg-end">{orderLotDetails?.price} USDC</td>
        </tr>
      )}

      {orderLotDetails !== null && (
        <tr>
          <td>Size</td>
          <td className="text-lg-end">{orderLotDetails?.size}</td>
        </tr>
      )}
    </InstructionCard>
  );
}
