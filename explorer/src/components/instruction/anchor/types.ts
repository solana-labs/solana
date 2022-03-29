import { Connection } from "@solana/web3.js";
import { TransactionInstruction } from "@solana/web3.js";
import { Program, Provider } from "@project-serum/anchor";

export const isInstructionFromAnAnchorProgram = (
  url: string,
  ix: TransactionInstruction
) => {
  const checkIdl = async () => {
    return Program.fetchIdl(ix.programId, {
      connection: new Connection(url),
    } as Provider)
      .then((_idl) => {
        return true;
      })
      .catch((_err) => {
        return false;
      });
  };
  return checkIdl();
};
