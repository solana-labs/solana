import { TransactionInstruction } from "@solana/web3.js";

const PROGRAM_ID: string = "AddressLookupTab1e1111111111111111111111111";

const INSTRUCTION_LOOKUP: { [key: number]: string } = {
  0: "Create Lookup Table",
  1: "Freeze Lookup Table",
  2: "Extend Lookup Table",
  3: "Deactivate Lookup Table",
  4: "Close Lookup Table",
};

export function isAddressLookupTableInstruction(
  instruction: TransactionInstruction
): boolean {
  return PROGRAM_ID === instruction.programId.toBase58();
}

export function parseAddressLookupTableInstructionTitle(
  instruction: TransactionInstruction
): string {
  const code = instruction.data[0];

  if (!(code in INSTRUCTION_LOOKUP)) {
    throw new Error(
      `Unrecognized Address Lookup Table instruction code: ${code}`
    );
  }

  return INSTRUCTION_LOOKUP[code];
}
