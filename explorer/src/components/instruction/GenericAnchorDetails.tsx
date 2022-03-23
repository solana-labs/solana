import {
  Connection,
  SignatureResult,
  TransactionInstruction,
} from "@solana/web3.js";
import { InstructionCard } from "./InstructionCard";
import {
  BorshInstructionCoder,
  Idl,
  Program,
  Provider,
} from "@project-serum/anchor";
import React, { useEffect, useState } from "react";
import { useCluster } from "../../providers/cluster";
import { Address } from "../common/Address";
import { snakeCase } from "snake-case";

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

  const [idl, setIdl] = useState<Idl | null>();
  useEffect(() => {
    async function fetchIdl() {
      if (idl) {
        return;
      }

      // fetch on chain idl
      const idl_: Idl | null = await Program.fetchIdl(ix.programId, {
        connection: new Connection(cluster.url),
      } as Provider);
      setIdl(idl_);
    }

    fetchIdl();
  }, [ix.programId, cluster.url, idl]);

  const [programName, setProgramName] = useState<string | null>(null);
  const [ixTitle, setIxTitle] = useState<string | null>(null);
  const [ixAccounts, setIxAccounts] = useState<
    { name: string; isMut: boolean; isSigner: boolean; pda?: Object }[] | null
  >(null);

  useEffect(() => {
    async function parseIxDetailsUsingCoder() {
      if (!idl || (programName && ixTitle && ixAccounts)) {
        return;
      }

      // e.g. voter_stake_registry -> voter stake registry
      var _programName = idl.name.replaceAll("_", " ").trim();
      // e.g. voter stake registry -> Voter Stake Registry
      _programName = _programName
        .toLowerCase()
        .split(" ")
        .map((word) => word.charAt(0).toUpperCase() + word.substring(1))
        .join(" ");
      setProgramName(_programName);

      const coder = new BorshInstructionCoder(idl);
      const decodedIx = coder.decode(ix.data);
      if (!decodedIx) {
        return;
      }

      // get ix title, pascal case it
      var _ixTitle = decodedIx.name;
      _ixTitle = _ixTitle.charAt(0).toUpperCase() + _ixTitle.slice(1);
      setIxTitle(_ixTitle);

      // get ix accounts
      const idlInstructions = idl.instructions.filter(
        (ix) => ix.name === decodedIx.name
      );
      if (idlInstructions.length === 0) {
        return;
      }
      setIxAccounts(
        idlInstructions[0].accounts as {
          // type coercing since anchor doesn't export the underlying type
          name: string;
          isMut: boolean;
          isSigner: boolean;
          pda?: Object;
        }[]
      );
    }

    parseIxDetailsUsingCoder();
  }, [
    ix.programId,
    ix.keys,
    ix.data,
    idl,
    cluster,
    programName,
    ixTitle,
    ixAccounts,
  ]);

  return (
    <div>
      {idl && (
        <InstructionCard
          ix={ix}
          index={index}
          result={result}
          title={`${programName || "Unknown"}: ${ixTitle || "Unknown"}`}
          innerCards={innerCards}
          childIndex={childIndex}
        >
          <tr key={ix.programId.toBase58()}>
            <td>Program</td>
            <td className="text-lg-end">
              <Address pubkey={ix.programId} alignRight link />
            </td>
          </tr>

          {ixAccounts != null &&
            ix.keys.map((am, keyIndex) => (
              <tr key={keyIndex}>
                <td>
                  <div className="me-2 d-md-inline">
                    {/* remaining accounts would not have a name */}
                    {ixAccounts[keyIndex] &&
                      snakeCase(ixAccounts[keyIndex].name)}
                    {!ixAccounts[keyIndex] &&
                      "remaining account #" +
                        (keyIndex - ixAccounts.length + 1)}
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
      )}
      {!idl && (
        <InstructionCard
          ix={ix}
          index={index}
          result={result}
          title={`Unknown Program: Unknown Instruction`}
          innerCards={innerCards}
          childIndex={childIndex}
          defaultRaw
        />
      )}
    </div>
  );
}
