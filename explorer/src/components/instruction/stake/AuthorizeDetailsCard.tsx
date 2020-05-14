import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  StakeInstruction,
  StakeProgram
} from "@solana/web3.js";
import { displayAddress } from "utils/tx";
import { InstructionCard } from "../InstructionCard";
import Copyable from "components/Copyable";
import { UnknownDetailsCard } from "../UnknownDetailsCard";

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

  const stakePubkey = params.stakePubkey.toBase58();
  const authorizedPubkey = params.authorizedPubkey.toBase58();
  const newAuthorizedPubkey = params.newAuthorizedPubkey.toBase58();

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
        <td className="text-right">
          <Copyable bottom text={StakeProgram.programId.toBase58()}>
            <code>{displayAddress(StakeProgram.programId.toBase58())}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Stake Address</td>
        <td className="text-right">
          <Copyable text={stakePubkey}>
            <code>{stakePubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Old Authority Address</td>
        <td className="text-right">
          <Copyable text={authorizedPubkey}>
            <code>{authorizedPubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>New Authority Address</td>
        <td className="text-right">
          <Copyable text={newAuthorizedPubkey}>
            <code>{newAuthorizedPubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Authority Type</td>
        <td className="text-right">{authorizationType}</td>
      </tr>
    </InstructionCard>
  );
}
