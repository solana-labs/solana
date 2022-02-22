import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import BN from "bn.js";
import { Address } from "components/common/Address";
import { useCluster } from "providers/cluster";
import { useEffect, useState } from "react";
import { InstructionCard } from "../InstructionCard";
import {
  getPerpMarketFromInstruction,
  getPerpMarketFromPerpMarketConfig,
  OrderLotDetails,
  PlacePerpOrder2,
} from "./types";

export function PlacePerpOrder2DetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: PlacePerpOrder2;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;
  const mangoAccount = ix.keys[1];
  const perpMarketAccountMeta = ix.keys[4];
  const mangoPerpMarketConfig = getPerpMarketFromInstruction(
    ix,
    perpMarketAccountMeta
  );

  const cluster = useCluster();
  const [orderLotDetails, setOrderLotDetails] =
    useState<OrderLotDetails | null>(null);
  useEffect(() => {
    async function getOrderLotDetails() {
      if (mangoPerpMarketConfig === undefined) {
        return;
      }
      const mangoPerpMarket = await getPerpMarketFromPerpMarketConfig(
        cluster.url,
        mangoPerpMarketConfig
      );
      const maxBaseQuantity = mangoPerpMarket.baseLotsToNumber(
        new BN(info.maxBaseQuantity.toString())
      );
      const limitPrice = mangoPerpMarket.priceLotsToNumber(
        new BN(info.price.toString())
      );
      setOrderLotDetails({
        price: limitPrice,
        size: maxBaseQuantity,
      } as OrderLotDetails);
    }
    getOrderLotDetails();
  }, [cluster.url, info.maxBaseQuantity, info.price, mangoPerpMarketConfig]);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango Program: PlacePerpOrder2"
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
      {mangoPerpMarketConfig !== undefined && (
        <tr>
          <td>Perp market</td>
          <td className="text-lg-end">{mangoPerpMarketConfig.name}</td>
        </tr>
      )}
      <tr>
        <td>Perp market address</td>
        <td>
          <Address pubkey={perpMarketAccountMeta.pubkey} alignRight link />
        </td>
      </tr>
      {info.clientOrderId !== "0" && (
        <tr>
          <td>Client order Id</td>
          <td className="text-lg-end">{info.clientOrderId}</td>
        </tr>
      )}
      <tr>
        <td>Order type</td>
        <td className="text-lg-end">{info.orderType}</td>
      </tr>
      <tr>
        <td>Side</td>
        <td className="text-lg-end">{info.side}</td>
      </tr>
      {orderLotDetails !== null && (
        <tr>
          <td>price</td>
          <td className="text-lg-end">{orderLotDetails?.price} USDC</td>
        </tr>
      )}
      {orderLotDetails !== null && (
        <tr>
          <td>quantity</td>
          <td className="text-lg-end">{orderLotDetails?.size}</td>
        </tr>
      )}
      <tr>
        <td>Reduce only</td>
        <td className="text-lg-end">{info.reduceOnly}</td>
      </tr>

      <tr>
        <td>Expiry timestamp</td>
        <td className="text-lg-end">{info.expiryTimestamp}</td>
      </tr>
    </InstructionCard>
  );
}
