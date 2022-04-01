import React from "react";
import {
  SignatureResult,
  ParsedInstruction,
  ParsedTransaction,
  BPF_LOADER_PROGRAM_ID,
} from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";
import { create } from "superstruct";
import { ParsedInfo } from "validators";
import { WriteInfo, FinalizeInfo } from "./types";
import { reportError } from "utils/sentry";
import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { Address } from "components/common/Address";
import { wrap } from "utils";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function BpfLoaderDetailsCard(props: DetailsProps) {
  try {
    const parsed = create(props.ix.parsed, ParsedInfo);

    switch (parsed.type) {
      case "write": {
        const info = create(parsed.info, WriteInfo);
        return <BpfLoaderWriteDetailsCard info={info} {...props} />;
      }
      case "finalize": {
        const info = create(parsed.info, FinalizeInfo);
        return <BpfLoaderFinalizeDetailsCard info={info} {...props} />;
      }
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

type Props<T> = {
  ix: ParsedInstruction;
  index: number;
  result: SignatureResult;
  info: T;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function BpfLoaderWriteDetailsCard(props: Props<WriteInfo>) {
  const { ix, index, result, info, innerCards, childIndex } = props;
  const bytes = wrap(info.bytes, 50);
  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="BPF Loader 2: Write"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={BPF_LOADER_PROGRAM_ID} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.account} alignRight link />
        </td>
      </tr>

      <tr>
        <td>
          Bytes <span className="text-muted">(Base 64)</span>
        </td>
        <td className="text-lg-end">
          <pre className="d-inline-block text-start mb-0">{bytes}</pre>
        </td>
      </tr>

      <tr>
        <td>Offset</td>
        <td className="text-lg-end">{info.offset}</td>
      </tr>
    </InstructionCard>
  );
}

export function BpfLoaderFinalizeDetailsCard(props: Props<FinalizeInfo>) {
  const { ix, index, result, info, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="BPF Loader 2: Finalize"
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <tr>
        <td>Program</td>
        <td className="text-lg-end">
          <Address pubkey={BPF_LOADER_PROGRAM_ID} alignRight link />
        </td>
      </tr>

      <tr>
        <td>Account</td>
        <td className="text-lg-end">
          <Address pubkey={info.account} alignRight link />
        </td>
      </tr>
    </InstructionCard>
  );
}
