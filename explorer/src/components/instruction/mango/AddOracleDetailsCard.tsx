import { SignatureResult, TransactionInstruction } from "@solana/web3.js";
import { InstructionCard } from "../InstructionCard";

export function AddOracleDetailsCard(props: {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  innerCards?: JSX.Element[];
  childIndex?: number;
}) {
  const { ix, index, result, innerCards, childIndex } = props;

  return (
    <InstructionCard
      ix={ix}
      index={index}
      result={result}
      title="Mango Program: AddOracle"
      innerCards={innerCards}
      childIndex={childIndex}
    ></InstructionCard>
  );
}
