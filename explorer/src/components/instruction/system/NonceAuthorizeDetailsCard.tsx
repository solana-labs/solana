import React from "react";
import {
  SystemProgram,
  SignatureResult,
  ParsedInstruction,
} from "@safecoin/web3.js";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { AuthorizeNonceInfo } from "./types";

export function NonceAuthorizeDetailsCard(props: {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: AuthorizeNonceInfo;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Authorize Nonce"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={SystemProgram.programId} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Nonce Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.nonceAccount} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Old Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.nonceAuthority} alignRight link />
        </td>
      </tr>

      <tr>
        <td>New Authority Address</td>
        <td className="text-lg-right">
          <Address pubkey={info.newAuthorized} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
