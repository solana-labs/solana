import React from "react";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { InitializeMarket, SerumIxDetailsProps } from "./types";

export function InitializeMarketDetailsCard(
  props: SerumIxDetailsProps<InitializeMarket>
) {
  const { ix, index, result, programName, info, innerCards, childIndex } =
    props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName} Program: Initialize Market`}
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={info.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Market</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Request Queue</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.requestQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Event Queue</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.eventQueue} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Bids</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.bids} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Asks</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.asks} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Vault</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.baseVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Quote Vault</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.quoteVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Mint</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.baseMint} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Quote Mint</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.quoteMint} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Lot Size</td>
        <td className="text-lg-end">{info.data.baseLotSize.toString(10)}</td>
      </tr>

      <tr>
        <td>Quote Lot Size</td>
        <td className="text-lg-end">{info.data.quoteLotSize.toString(10)}</td>
      </tr>

      <tr>
        <td>Fee Rate Bps</td>
        <td className="text-lg-end">{info.data.feeRateBps}</td>
      </tr>

      <tr>
        <td>Quote Dust Threshold</td>
        <td className="text-lg-end">
          {info.data.quoteDustThreshold.toString(10)}
        </td>
      </tr>

      <tr>
        <td>Vault Signer Nonce</td>
        <td className="text-lg-end">
          {info.data.vaultSignerNonce.toString(10)}
        </td>
      </tr>
    </InstructionCard>
  );
}
