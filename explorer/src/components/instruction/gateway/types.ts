import {TransactionInstruction} from "@solana/web3.js";
import {GATEWAY_PROGRAM_ID} from "../../../providers/accounts";

export const isGatewayInstruction = (instruction: TransactionInstruction) => {
  console.log("isGatewayIstruction", GATEWAY_PROGRAM_ID.equals(instruction.programId));
  return GATEWAY_PROGRAM_ID.equals(instruction.programId);
}
