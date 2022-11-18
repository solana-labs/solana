import React from "react";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { SettleFunds, SerumIxDetailsProps } from "./types";

export function SettleFundsDetailsCard(
  props: SerumIxDetailsProps<SettleFunds>
) {
  const { ix, index, result, programName, info, innerCards, childIndex } =
    props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName} Program: Settle Funds`}
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
        <td>Open Orders</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.openOrders} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Open Orders Owner</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.openOrdersOwner} alignRight link />
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
        <td>Base Wallet</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.baseWallet} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Quote Wallet</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.quoteWallet} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Vault Signer</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.vaultSigner} alignRight link />
        </td>
      </tr>

      {info.accounts.referrerQuoteWallet && (
        <tr>
          <td>Referrer Quote Wallet</td>
          <td className="text-lg-end">
            <Address
              pubkey={info.accounts.referrerQuoteWallet}
              alignRight
              link
            />
          </td>
        </tr>
      )}
    </InstructionCard>
  );
}
