import React from "react";
import { coerce } from "superstruct";
import {
  SignatureResult,
  ParsedTransaction,
  PublicKey,
  ParsedInstruction,
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { IX_STRUCTS, TokenInstructionType } from "./types";
import { ParsedInfo } from "validators";

const IX_TITLES = {
  initializeMint: "Initialize Mint",
  initializeAccount: "Initialize Account",
  initializeMultisig: "Initialize Multisig",
  transfer: "Transfer",
  approve: "Approve",
  revoke: "Revoke",
  setOwner: "Set Owner",
  mintTo: "Mint To",
  burn: "Burn",
  closeAccount: "Close Account",
};

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
};

export function TokenDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);
    const { type: rawType, info } = parsed;
    const type = coerce(rawType, TokenInstructionType);
    const title = `Token: ${IX_TITLES[type]}`;
    const coerced = coerce(info, IX_STRUCTS[type] as any);
    return <TokenInstruction title={title} info={coerced} {...props} />;
  } catch (err) {
    return <UnknownDetailsCard {...props} />;
  }
}

type InfoProps = {
  ix: ParsedInstruction;
  info: any;
  result: SignatureResult;
  index: number;
  title: string;
};

function TokenInstruction(props: InfoProps) {
  const attributes = [];
  for (let key in props.info) {
    const value = props.info[key];
    if (value === undefined) continue;

    let tag;
    if (value instanceof PublicKey) {
      tag = <Address pubkey={value} alignRight link />;
    } else {
      tag = <>{value}</>;
    }

    key = key.charAt(0).toUpperCase() + key.slice(1);

    attributes.push(
      <tr key={key}>
        <td>{key}</td>
        <td className="text-lg-right">{tag}</td>
      </tr>
    );
  }

  return (
    <InstructionCard
      ix={props.ix}
      index={props.index}
      result={props.result}
      title={props.title}
    >
      {attributes}
    </InstructionCard>
  );
}
