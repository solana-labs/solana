import React from "react";

type Props = {
  slot: number;
};
export function Slot({ slot }: Props) {
  return <span className="text-monospace">{slot.toLocaleString("en-US")}</span>;
}
