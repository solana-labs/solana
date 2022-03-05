import {
  Connection,
  SignatureResult,
  TransactionInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import {
  BorshInstructionCoder,
  Program,
  Provider,
} from "@project-serum/anchor";
import React, { useEffect, useState } from "react";
import { useCluster } from "../../providers/cluster";
import { Address } from "../common/Address";
import { snakeCase } from "snake-case";
import { Idl } from "@project-serum/anchor";

export function GenericAnchorDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  signature: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, innerCards, childIndex } = props;

  const cluster = useCluster();

  const [programName, setProgramName] = useState<string | null>(null);
  const [ixTitle, setIxTitle] = useState<string | null>(null);
  const [ixAccounts, setIxAccounts] = useState<
    { name: string; isMut: boolean; isSigner: boolean; pda?: Object }[] | null
  >(null);

  useEffect(() => {
    async function load() {
      // fetch on chain idl
      const idl: Idl | null = await Program.fetchIdl(ix.programId, {
        connection: new Connection(cluster.url),
      } as Provider);

      if (!idl) {
        return;
      }

      // e.g. voter_stake_registry -> voter stake registry
      const programName = idl.name.replaceAll("_", " ").trim();
      setProgramName(programName);

      const coder = new BorshInstructionCoder(idl);
      const decodedIx = coder.decode(ix.data);
      if (!decodedIx) {
        return;
      }

      // get ix title
      setIxTitle(decodedIx.name);

      // get ix accounts
      const idlInstructions = idl.instructions.filter(
        (ix) => ix.name === decodedIx.name
      );
      if (idlInstructions.length === 0) {
        return;
      }
      const ixAccounts = idlInstructions[0].accounts;
      setIxAccounts(
        ixAccounts as {
          // type coercing since anchor doesn't export the underlying type
          name: string;
          isMut: boolean;
          isSigner: boolean;
          pda?: Object;
        }[]
      );
    }

    load();
  }, [ix.programId, ix.data, cluster]);

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title={`${programName || "Unknown"}: ${snakeCase(ixTitle || "Unknown")}`}
      innerCards={innerCards}
      childIndex={childIndex}
    >
      <td>Program</td>
      <td className="text-lg-end">
        <Address pubkey={ix.programId} alignRight link />
      </td>

      {ixAccounts != null &&
        ix.keys.map((am, keyIndex) => (
          <tr>
            <td>
              <div className="me-2 d-md-inline">
                {snakeCase(ixAccounts[keyIndex].name)}
              </div>
              {am.isWritable && (
                <span className="badge bg-info-soft me-1">Writable</span>
              )}
              {am.isSigner && (
                <span className="badge bg-info-soft me-1">Signer</span>
              )}
            </td>
            <td>
              <Address pubkey={am.pubkey} alignRight link />
            </td>
          </tr>
        ))}
    </InstructionCard>
  );
}
