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

  const stakerPubkey = params.authorized.staker.toBase58();
  const withdrawerPubkey = params.authorized.withdrawer.toBase58();
  const stakePubkey = params.stakePubkey.toBase58();

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
          <Copyable bottom text={StakeProgram.programId.toBase58()}>
            <code>{displayAddress(StakeProgram.programId)}</code>
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
        <td>Authorized Staker Address</td>
        <td className="text-right">
          <Copyable text={stakerPubkey}>
            <code>{stakerPubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Authorized Withdrawer Address</td>
        <td className="text-right">
          <Copyable text={withdrawerPubkey}>
            <code>{withdrawerPubkey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Lockup Expiry Epoch</td>
        <td className="text-right">{params.lockup.epoch}</td>
      </tr>

      <tr>
        <td>Lockup Expiry Timestamp</td>
        <td className="text-right">
          {new Date(params.lockup.unixTimestamp * 1000).toUTCString()}
        </td>
      </tr>

      <tr>
        <td>Lockup Custodian Address</td>
        <td className="text-right">
          <Copyable text={params.lockup.custodian.toBase58()}>
            <code>{displayAddress(params.lockup.custodian)}</code>
          </Copyable>
        </td>
      </tr>
    </InstructionCard>
  );
}
