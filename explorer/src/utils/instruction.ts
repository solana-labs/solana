import { create } from "superstruct";
import {
  IX_TITLES,
  TokenInstructionType,
} from "components/instruction/token/types";
import { ParsedInfo } from "validators";
import { reportError } from "utils/sentry";
import {
  ConfirmedSignatureInfo,
  ParsedTransactionWithMeta,
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
import {
  isBonfidaBotInstruction,
  parseBonfidaBotInstructionTitle,
} from "components/instruction/bonfida-bot/types";
import { TOKEN_PROGRAM_ID } from "providers/accounts/tokens";

export type InstructionType = {
  name: string;
  innerInstructions: (ParsedInstruction | PartiallyDecodedInstruction)[];
};

export interface InstructionItem {
  instruction: ParsedInstruction | PartiallyDecodedInstruction;
  inner: (ParsedInstruction | PartiallyDecodedInstruction)[];
}

export class InstructionContainer {
  readonly instructions: InstructionItem[];

  static create(transactionWithMeta: ParsedTransactionWithMeta) {
    return new InstructionContainer(transactionWithMeta);
  }

  constructor(transactionWithMeta: ParsedTransactionWithMeta) {
    this.instructions =
      transactionWithMeta.transaction.message.instructions.map(
        (instruction) => {
          if ("parsed" in instruction) {
            if (typeof instruction.parsed === "object") {
              instruction.parsed = create(instruction.parsed, ParsedInfo);
            } else if (typeof instruction.parsed !== "string") {
              throw new Error("Unexpected parsed response");
            }
          }

          return {
            instruction,
            inner: [],
          };
        }
      );

    if (transactionWithMeta.meta?.innerInstructions) {
      for (let inner of transactionWithMeta.meta.innerInstructions) {
        this.instructions[inner.index].inner.push(...inner.instructions);
      }
    }
  }
}

export function getTokenProgramInstructionName(
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

export function getTokenInstructionName(
  transactionWithMeta: ParsedTransactionWithMeta,
  ix: ParsedInstruction | PartiallyDecodedInstruction,
  signatureInfo: ConfirmedSignatureInfo
) {
  let name = "Unknown";

  let transactionInstruction;
  if (transactionWithMeta?.transaction) {
    transactionInstruction = intoTransactionInstruction(
      transactionWithMeta.transaction,
      ix
    );
  }

  if ("parsed" in ix) {
    if (ix.program === "spl-token") {
      name = getTokenProgramInstructionName(ix, signatureInfo);
    } else {
      return undefined;
    }
  } else if (
    transactionInstruction &&
    isBonfidaBotInstruction(transactionInstruction)
  ) {
    try {
      name = parseBonfidaBotInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
      return undefined;
    }
  } else if (
    transactionInstruction &&
    isSerumInstruction(transactionInstruction)
  ) {
    try {
      name = parseSerumInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
      return undefined;
    }
  } else if (
    transactionInstruction &&
    isTokenSwapInstruction(transactionInstruction)
  ) {
    try {
      name = parseTokenSwapInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
      return undefined;
    }
  } else if (
    transactionInstruction &&
    isTokenLendingInstruction(transactionInstruction)
  ) {
    try {
      name = parseTokenLendingInstructionTitle(transactionInstruction);
    } catch (error) {
      reportError(error, { signature: signatureInfo.signature });
      return undefined;
    }
  } else {
    if (
      ix.accounts.findIndex((account) => account.equals(TOKEN_PROGRAM_ID)) >= 0
    ) {
      name = "Unknown (Inner)";
    } else {
      return undefined;
    }
  }

  return name;
}

export function getTokenInstructionType(
  transactionWithMeta: ParsedTransactionWithMeta,
  ix: ParsedInstruction | PartiallyDecodedInstruction,
  signatureInfo: ConfirmedSignatureInfo,
  index: number
): InstructionType | undefined {
  const innerInstructions: (ParsedInstruction | PartiallyDecodedInstruction)[] =
    [];

  if (transactionWithMeta.meta?.innerInstructions) {
    transactionWithMeta.meta.innerInstructions.forEach((ix) => {
      if (ix.index === index) {
        ix.instructions.forEach((inner) => {
          innerInstructions.push(inner);
        });
      }
    });
  }

  let name =
    getTokenInstructionName(transactionWithMeta, ix, signatureInfo) ||
    "Unknown";

  return {
    name,
    innerInstructions,
  };
}
