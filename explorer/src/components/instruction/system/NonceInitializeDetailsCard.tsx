import React from "react";
import {
  TransactionInstruction,
  SystemProgram,
  SignatureResult,
  SystemInstruction
} from "@solana/web3.js";
import { displayAddress } from "utils/tx";
import { InstructionCard } from "../InstructionCard";
import Copyable from "components/Copyable";
import { UnknownDetailsCard } from "../UnknownDetailsCard";

export function NonceInitializeDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
}) {
  const { ix, index, result } = props;

  let params;
  try {
    params = SystemInstruction.decodeNonceInitialize(ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  const nonceKey = params.noncePubkey.toBase58();
  const authorizedKey = params.authorizedPubkey.toBase58();
  const [nonceMeta] = ix.keys;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Initialize Nonce"
    >
      <tr>
        <td>Program</td>
        <td className="text-right">
          <Copyable bottom text={SystemProgram.programId.toBase58()}>
            <code>{displayAddress(SystemProgram.programId)}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>
          <div className="mr-2 d-md-inline">Nonce Address</div>
          {!nonceMeta.isWritable && (
            <span className="badge badge-soft-dark mr-1">Readonly</span>
          )}
          {nonceMeta.isSigner && (
            <span className="badge badge-soft-dark mr-1">Signer</span>
          )}
        </td>
        <td className="text-right">
          <Copyable text={nonceKey}>
            <code>{nonceKey}</code>
          </Copyable>
        </td>
      </tr>

      <tr>
        <td>Authorized Address</td>
        <td className="text-right">
          <Copyable text={authorizedKey}>
            <code>{authorizedKey}</code>
          </Copyable>
        </td>
      </tr>
    </InstructionCard>
  );
}
