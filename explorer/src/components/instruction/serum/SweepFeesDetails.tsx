import React from "react";
import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { SweepFees } from "./types";

export function SweepFeesDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: SweepFees;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Serum Program: Sweep Fees"
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
        <td>Quote Vault</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.quoteVault} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Fee Sweeping Authority</td>
        <td className="text-lg-end">
          <Address
            pubkey={info.accounts.feeSweepingAuthority}
            alignRight
            link
          />
        </td>
      </tr>

      <tr>
        <td>Fee Receiver</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.quoteFeeReceiver} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Vault Signer</td>
        <td className="text-lg-end">
          <Address pubkey={info.accounts.vaultSigner} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
