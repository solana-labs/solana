import React from "react";
import { Cluster } from "providers/cluster";
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { BorshInstructionCoder, Program } from "@project-serum/anchor";
import { useAnchorProgram } from "providers/anchor";
import { programLabel } from "utils/tx";
import { ErrorBoundary } from "@sentry/react";

function snakeToPascal(string: string) {
  return string
    .split("/")
    .map((snake) =>
      snake
        .split("_")
        .map((substr) => substr.charAt(0).toUpperCase() + substr.slice(1))
        .join("")
    )
    .join("/");
}

export function getProgramName(program: Program | null): string | undefined {
  return program ? snakeToPascal(program.idl.name) : undefined;
}

export function capitalizeFirstLetter(input: string) {
  return input.charAt(0).toUpperCase() + input.slice(1);
}

function AnchorProgramName({
  programId,
  url,
}: {
  programId: PublicKey;
  url: string;
}) {
  const program = useAnchorProgram(programId.toString(), url);
  if (!program) {
    throw new Error("No anchor program name found for given programId");
  }
  const programName = getProgramName(program);
  return <>{programName}</>;
}

export function ProgramName({
  programId,
  cluster,
  url,
}: {
  programId: PublicKey;
  cluster: Cluster;
  url: string;
}) {
  const defaultProgramName =
    programLabel(programId.toBase58(), cluster) || "Unknown Program";

  return (
    <React.Suspense fallback={defaultProgramName}>
      <ErrorBoundary fallback={<>{defaultProgramName}</>}>
        <AnchorProgramName programId={programId} url={url} />
      </ErrorBoundary>
    </React.Suspense>
  );
}

export function getAnchorNameForInstruction(
  ix: TransactionInstruction,
  program: Program
): string | null {
  const coder = new BorshInstructionCoder(program.idl);
  const decodedIx = coder.decode(ix.data);

  if (!decodedIx) {
    return null;
  }

  var _ixTitle = decodedIx.name;
  return _ixTitle.charAt(0).toUpperCase() + _ixTitle.slice(1);
}

export function getAnchorAccountsFromInstruction(
  decodedIx: Object | null,
  program: Program
):
  | {
      name: string;
      isMut: boolean;
      isSigner: boolean;
      pda?: Object;
    }[]
  | null {
  if (decodedIx) {
    // get ix accounts
    const idlInstructions = program.idl.instructions.filter(
      // @ts-ignore
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
