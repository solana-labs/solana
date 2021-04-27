import React from "react";
import {
  SignatureResult,
  TransactionInstruction,
  PublicKey,
} from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { CreateOrder } from "./types";

export function CreateOrderDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: CreateOrder;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;
  console.log("Test");
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Bonfida Bot: Create Order"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={info.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Signal Provider Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.signalProviderKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Market</td>
        <td className="text-lg-right">
          <Address pubkey={info.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Payer Bot Asset Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.payerPoolAssetKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Order</td>
        <td className="text-lg-right">
          <Address pubkey={info.openOrdersKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Request Queue</td>
        <td className="text-lg-right">
          <Address pubkey={info.serumRequestQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Event Queue</td>
        <td className="text-lg-right">
          <Address pubkey={info.serumEventQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Bids</td>
        <td className="text-lg-right">
          <Address pubkey={info.serumMarketBids} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Asks</td>
        <td className="text-lg-right">
          <Address pubkey={info.serumMarketAsks} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bot Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.poolKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Coin Vault</td>
        <td className="text-lg-right">
          <Address pubkey={info.coinVaultKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pc Vault</td>
        <td className="text-lg-right">
          <Address pubkey={info.pcVaultKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Serum Program ID</td>
        <td className="text-lg-right">
          <Address pubkey={info.dexProgramKey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bot Token Mint</td>
        <td className="text-lg-right">
          <Address pubkey={new PublicKey(info.targetMint)} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Pool Seed</td>
        <td className="text-lg-right">{info.poolSeed}</td>
      </tr>

      <tr>
        <td>Side</td>
        <td className="text-lg-right">{info.side}</td>
      </tr>

      <tr>
        <td>Limit Price</td>
        <td className="text-lg-right">{info.limitPrice}</td>
      </tr>

      <tr>
        <td>Ratio to Trade</td>
        <td className="text-lg-right">{info.ratioOfPoolAssetsToTrade}</td>
      </tr>

      <tr>
        <td>Order Type</td>
        <td className="text-lg-right">{info.orderType}</td>
      </tr>

      <tr>
        <td>Coin Lot Size</td>
        <td className="text-lg-right">{info.coinLotSize.toString()}</td>
      </tr>

      <tr>
        <td>Pc Lot Size</td>
        <td className="text-lg-right">{info.pcLotSize.toString()}</td>
      </tr>
    </InstructionCard>
  );
}
