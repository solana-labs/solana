export default function getInstructionCardScrollAnchorId(
  // An array of instruction sequence numbers, starting with the
  // top level instruction number. Instruction numbers start from 1.
  instructionNumberPath: number[]
): string {
  return `ix-${instructionNumberPath.join("-")}`;
}
