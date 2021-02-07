import React from "react";
import { SignatureResult, TransactionInstruction } from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { SettleFunds } from "./types";

export function SettleFundsDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: SettleFunds;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Serum: Settle Funds"
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
        <td>Market</td>
        <td className="text-lg-right">
          <Address pubkey={info.market} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders</td>
        <td className="text-lg-right">
          <Address pubkey={info.openOrders} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Owner</td>
        <td className="text-lg-right">
          <Address pubkey={info.owner} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Vault</td>
        <td className="text-lg-right">
          <Address pubkey={info.baseVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Quote Vault</td>
        <td className="text-lg-right">
          <Address pubkey={info.quoteVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Base Wallet</td>
        <td className="text-lg-right">
          <Address pubkey={info.baseWallet} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Quote Wallet</td>
        <td className="text-lg-right">
          <Address pubkey={info.quoteWallet} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Vault Signer</td>
        <td className="text-lg-right">
          <Address pubkey={info.vaultSigner} alignRight link />
        </td>
      </tr>

      {info.referrerQuoteWallet && (
        <tr>
          <td>Referrer Quote Wallet</td>
          <td className="text-lg-right">
            <Address pubkey={info.referrerQuoteWallet} alignRight link />
          </td>
        </tr>
      )}
    </InstructionCard>
  );
}
