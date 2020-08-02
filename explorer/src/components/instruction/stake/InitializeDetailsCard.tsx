import React from "react";
import {
  TransactionInstruction,
  SignatureResult,
  StakeInstruction,
  StakeProgram,
  SystemProgram,
} from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import Address from "components/common/Address";

export function InitializeDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = StakeInstruction.decodeInitialize(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Stake Initialize"
    >
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Address pubkey={StakeProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Stake Address</td>
        <td className="text-right">
          <Address pubkey={params.stakePubkey} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Stake Authority Address</td>
        <td className="text-right">
          <Address pubkey={params.authorized.staker} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Withdraw Authority Address</td>
        <td className="text-right">
          <Address pubkey={params.authorized.withdrawer} alignRight link />
        </td>
      </tr>

      {params.lockup.epoch > 0 && (
        <tr>
          <td>Lockup Expiry Epoch</td>
          <td className="text-right">{params.lockup.epoch}</td>
        </tr>
      )}

      {params.lockup.unixTimestamp > 0 && (
        <tr>
          <td>Lockup Expiry Timestamp</td>
          <td className="text-right">
            {new Date(params.lockup.unixTimestamp * 1000).toUTCString()}
          </td>
        </tr>
      )}

      {!params.lockup.custodian.equals(SystemProgram.programId) && (
        <tr>
          <td>Lockup Custodian Address</td>
          <td className="text-right">
            <Address pubkey={params.lockup.custodian} alignRight link />
          </td>
        </tr>
      )}
    </InstructionCard>
  );
}
