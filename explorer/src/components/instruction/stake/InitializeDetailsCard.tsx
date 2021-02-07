import React from "react";
import {
  SignatureResult,
  StakeProgram,
  SystemProgram,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { InitializeInfo } from "./types";

export function InitializeDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: InitializeInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Stake Initialize"
      innerCards={innerCards}
      childIndex={childIndex}
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
          <Address pubkey={info.stakeAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Stake Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.authorized.staker} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Withdraw Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.authorized.withdrawer} alignRight link />
        </td>
      </tr>

      {info.lockup.epoch > 0 && (
        <tr>
          <td>Lockup Expiry Epoch</td>
          <td className="text-lg-right">{info.lockup.epoch}</td>
        </tr>
      )}

      {info.lockup.unixTimestamp > 0 && (
        <tr>
          <td>Lockup Expiry Timestamp</td>
          <td className="text-lg-right">
            {new Date(info.lockup.unixTimestamp * 1000).toUTCString()}
          </td>
        </tr>
      )}

      {!info.lockup.custodian.equals(SystemProgram.programId) && (
        <tr>
          <td>Lockup Custodian Address</td>
          <td className="text-lg-right">
            <Address pubkey={info.lockup.custodian} alignRight link />
          </td>
        </tr>
      )}
    </InstructionCard>
  );
}
