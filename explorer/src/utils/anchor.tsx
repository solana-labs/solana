import React from 'react';
import { clusterUrl, Cluster, DEFAULT_CUSTOM_URL } from "providers/cluster";
import { PublicKey } from '@solana/web3.js';
import { Program } from "@project-serum/anchor";
import { useAnchorProgram } from 'providers/anchor';
import { programLabel } from "utils/tx";

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