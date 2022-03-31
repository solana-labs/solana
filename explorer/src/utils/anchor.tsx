import React from 'react';
import { clusterUrl, Cluster, DEFAULT_CUSTOM_URL } from "providers/cluster";
import { PublicKey, TransactionInstruction } from '@solana/web3.js';
import { BorshInstructionCoder, Program } from "@project-serum/anchor";
import { useAnchorAccount, useAnchorProgram } from 'providers/anchor';
import { programLabel } from "utils/tx";
import { ErrorBoundary } from '@sentry/react';

export function getProgramName(program: Program | null): string | undefined {
  return program ? capitalizeFirstLetter(program.idl.name) : undefined
}

export function capitalizeFirstLetter(input: string) {
  return input.charAt(0).toUpperCase() + input.slice(1);
}

function AnchorProgramName({
  programId,
  cluster,
}: {
  programId: PublicKey,
  cluster: Cluster
}) {
  let url = clusterUrl(cluster, DEFAULT_CUSTOM_URL);
  const program = useAnchorProgram(programId.toString(), url);
  return <>{getProgramName(program)}</>
}

export function ProgramName({
  programId,
  cluster,
}: {
  programId: PublicKey,
  cluster: Cluster
}) {
  const defaultProgramName = (programLabel(programId.toBase58(), cluster) || "Unknown Program");

  return (
    <React.Suspense fallback={defaultProgramName}>
      <AnchorProgramName programId={programId} cluster={cluster} />
    </React.Suspense>
  )
}

function AnchorAccountName({
  accountPubkey,
  programId,
  cluster,
}: {
  accountPubkey: PublicKey,
  programId: PublicKey,
  cluster: Cluster,
}) {
  let url = clusterUrl(cluster, DEFAULT_CUSTOM_URL);
  const account = useAnchorAccount(accountPubkey.toString(), programId.toString(), url);
  if (!account) throw new Error("Unable to decode anchor account for pubkey");
  console.log("Account:", account.layout);

  return <>{camelToUnderscore(account.layout)}</>
}

export function AccountName({
  accountPubkey,
  programId,
  cluster,
  defaultAccountName
}: {
  accountPubkey: PublicKey,
  programId: PublicKey,
  cluster: Cluster,
  defaultAccountName: string
}) {
  return (<React.Suspense fallback={<>{defaultAccountName}</>}>
    <ErrorBoundary fallback={<>{defaultAccountName}</>}>
      <AnchorAccountName programId={programId} cluster={cluster} accountPubkey={accountPubkey} />
    </ErrorBoundary>
  </React.Suspense>)
}

export function getAnchorNameForInstruction(ix: TransactionInstruction, program: Program): string | null {
  const coder = new BorshInstructionCoder(program.idl);
  const decodedIx = coder.decode(ix.data);

  if (!decodedIx) { return null }

  var _ixTitle = decodedIx.name;
  return _ixTitle.charAt(0).toUpperCase() + _ixTitle.slice(1);
}

export function getAnchorAccountsFromInstruction(ix: TransactionInstruction, program: Program): {
  name: string;
  isMut: boolean;
  isSigner: boolean;
  pda?: Object;
}[] | null {
  const coder = new BorshInstructionCoder(program.idl);
  const decodedIx = coder.decode(ix.data);

  if (decodedIx) {
    // get ix accounts
    const idlInstructions = program.idl.instructions.filter(
      (ix) => ix.name === decodedIx.name
    );
    if (idlInstructions.length === 0) {
      return null;
    }
    return idlInstructions[0].accounts as {
      // type coercing since anchor doesn't export the underlying type
      name: string;
      isMut: boolean;
      isSigner: boolean;
      pda?: Object;
    }[];
  }
  return null;
}

function camelToUnderscore(key: string) {
  var result = key.replace(/([A-Z])/g, " $1");
  return result.split(" ").join("_").toLowerCase();
}
