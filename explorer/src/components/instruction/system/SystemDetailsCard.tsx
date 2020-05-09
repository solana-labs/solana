import React from "react";
import {
  SystemInstruction,
  TransactionInstruction,
  SignatureResult
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { TransferDetailsCard } from "./TransferDetailsCard";
import { AssignDetailsCard } from "./AssignDetailsCard";
import { CreateDetailsCard } from "./CreateDetailsCard";
import { CreateWithSeedDetailsCard } from "./CreateWithSeedDetailsCard";
import { NonceInitializeDetailsCard } from "./NonceInitializeDetailsCard";
import { NonceAdvanceDetailsCard } from "./NonceAdvanceDetailsCard";
import { NonceWithdrawDetailsCard } from "./NonceWithdrawDetailsCard";
import { NonceAuthorizeDetailsCard } from "./NonceAuthorizeDetailsCard";

type DetailsProps = {
  ix: TransactionInstruction;
  result: SignatureResult;
  index: number;
};

export function SystemDetailsCard(props: DetailsProps) {
  let systemInstructionType;
  try {
    systemInstructionType = SystemInstruction.decodeInstructionType(props.ix);
  } catch (err) {
    console.error(err);
    return <UnknownDetailsCard {...props} />;
  }

  switch (systemInstructionType) {
    case "Create":
      return <CreateDetailsCard {...props} />;
    case "Assign":
      return <AssignDetailsCard {...props} />;
    case "Transfer":
      return <TransferDetailsCard {...props} />;
    case "CreateWithSeed":
      return <CreateWithSeedDetailsCard {...props} />;
    case "AdvanceNonceAccount":
      return <NonceAdvanceDetailsCard {...props} />;
    case "WithdrawNonceAccount":
      return <NonceWithdrawDetailsCard {...props} />;
    case "AuthorizeNonceAccount":
      return <NonceAuthorizeDetailsCard {...props} />;
    case "InitializeNonceAccount":
      return <NonceInitializeDetailsCard {...props} />;
    default:
      return <UnknownDetailsCard {...props} />;
  }
}
