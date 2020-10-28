import React from "react";
import {
  SignatureResult,
  ParsedInstruction,
  ParsedTransaction,
  BPF_LOADER_PROGRAM_ID,
} from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { coerce } from "superstruct";
import { ParsedInfo } from "validators";
import { IX_STRUCTS } from "./types";
import { reportError } from "utils/sentry";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { Address } from "components/common/Address";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
};

export function BpfLoaderDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);
    const info = coerce(parsed.info, IX_STRUCTS[parsed.type]);

    switch (parsed.type) {
      case "write":
        return <BpfLoaderWriteDetailsCard info={info} {...props} />;
      case "finalize":
        return <BpfLoaderFinalizeDetailsCard info={info} {...props} />;
      default:
        return <UnknownDetailsCard {...props} />;
    }
  } catch (error) {
    reportError(error, {
      signature: props.tx.signatures[0],
    });
    return <UnknownDetailsCard {...props} />;
  }
}

type Props = {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: any;
};

export function BpfLoaderWriteDetailsCard(props: Props) {
  const { ix, index, result, info } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="BPF Loader 2: Write"
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={BPF_LOADER_PROGRAM_ID} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Account</td>
        <td className="text-lg-right">
          <Address pubkey={info.account} alignRight link />
        </td>
      </tr>

      <tr>
        <td>
          Bytes <span className="text-muted">(base 64)</span>
        </td>
        <td className="text-lg-right">
          <code className="d-inline-block">{info.bytes}</code>
        </td>
      </tr>

      <tr>
        <td>Offset</td>
        <td className="text-lg-right">{info.offset}</td>
      </tr>
    </InstructionCard>
  );
}

export function BpfLoaderFinalizeDetailsCard(props: Props) {
  const { ix, index, result, info } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="BPF Loader 2: Finalize"
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-right">
          <Address pubkey={BPF_LOADER_PROGRAM_ID} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Account</td>
        <td className="text-lg-right">
          <Address pubkey={info.account} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
