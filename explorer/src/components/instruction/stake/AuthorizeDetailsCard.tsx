import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  StakeInstruction,
  StakeProgram,
} from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import Address from "components/common/Address";

export function AuthorizeDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = StakeInstruction.decodeAuthorize(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  let authorizationType;
  switch (params.stakeAuthorizationType.index) {
    case 0:
      authorizationType = "Staker";
      break;
    case 1:
      authorizationType = "Withdrawer";
      break;
    default:
      authorizationType = "Invalid";
      break;
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Stake Authorize"
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={StakeProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Stake Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.stakePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Old Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.authorizedPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>New Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={params.newAuthorizedPubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Authority Type</td>
        <td className="text-lg-right">{authorizationType}</td>
      </tr>
    </InstructionCard>
  );
}
