import React from "react";
import {
  TransactionInstruction,
  SystemProgram,
  SignatureResult,
  SystemInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { Address } from "components/common/Address";

export function AssignDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = SystemInstruction.decodeAssign(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Assign Account"
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={SystemProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Account Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.accountPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Assigned Owner</td>
        <td className="text-lg-right">
          <Address pubkey={params.programId} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
