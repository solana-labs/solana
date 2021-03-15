import { create } from "superstruct";
import {
  IX_TITLES,
  TokenInstructionType,
} from "components/instruction/token/types";
import { ParsedInfo } from "validators";
import { reportError } from "utils/sentry";
import {
  ConfirmedSignatureInfo,
  ParsedConfirmedTransaction,
  ParsedInstruction,
  PartiallyDecodedInstruction,
} from "@solana/web3.js";
import { intoTransactionInstruction } from "utils/tx";
import {
  isTokenSwapInstruction,
  parseTokenSwapInstructionTitle,
} from "components/instruction/token-swap/types";
import {
  isTokenLendingInstruction,
  parseTokenLendingInstructionTitle,
} from "components/instruction/token-lending/types";
import {
  isSerumInstruction,
  parseSerumInstructionTitle,
} from "components/instruction/serum/types";
import { TOKEN_PROGRAM_ID } from "providers/accounts/tokens";

export type InstructionType = {
  name: string;
  innerInstructions: (ParsedInstruction | PartiallyDecodedInstruction)[];
};

export function getTokenInstructionName(
  ix: ParsedInstruction,
  signatureInfo: ConfirmedSignatureInfo
): string {
  try {
    const parsed = create(ix.parsed, ParsedInfo);
    const { type: rawType } = parsed;
    const type = create(rawType, TokenInstructionType);
    return IX_TITLES[type];
  } catch (err) {
    reportError(err, { signature: signatureInfo.signature });
    return "Unknown";
  }
}

export function getInstructionType(
  transaction: ParsedConfirmedTransaction,
  ix: ParsedInstruction | PartiallyDecodedInstruction,
  signatureInfo: ConfirmedSignatureInfo,
  index: number
): InstructionType {
  let name = "Unknown";

  const innerInstructions: (
    | ParsedInstruction
    | PartiallyDecodedInstruction
  )[] = [];

  if (transaction.meta?.innerInstructions) {
    transaction.meta.innerInstructions.forEach((ix) => {
      if (ix.index === index) {
        ix.instructions.forEach((inner) => {
          innerInstructions.push(inner);
        });
      }
    });
  }

  let transactionInstruction;
  if (transaction?.transaction) {
    transactionInstruction = intoTransactionInstruction(
      transaction.transaction,
      ix
    );
  }

  if ("parsed" in ix) {
    if (ix.program === "spl-token") {
      name = getTokenInstructionName(ix, signatureInfo);
    }
  } else if (
    transactionInstruction &&
    isSerumInstruction(transactionInstruction)
  ) {
    try {
      name = parseSerumInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
    }
  } else if (
    transactionInstruction &&
    isTokenSwapInstruction(transactionInstruction)
  ) {
    try {
      name = parseTokenSwapInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
    }
  } else if (
    transactionInstruction &&
    isTokenLendingInstruction(transactionInstruction)
  ) {
    try {
      name = parseTokenLendingInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
    }
  } else {
    if (
      ix.accounts.findIndex((account) => account.equals(TOKEN_PROGRAM_ID)) >= 0
    ) {
      name = "Unknown (Inner)";
    }
  }

  return {
    name,
    innerInstructions,
  };
}
