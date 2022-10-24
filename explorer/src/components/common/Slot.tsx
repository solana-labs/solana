import React from "react";
import Link from "next/link";
import { useCreateClusterPath } from "utils/routing";
import { Copyable } from "./Copyable";

type Props = {
  slot: number;
  link?: boolean;
};
export function Slot({ slot, link }: Props) {
  const createClusterPath = useCreateClusterPath();

  return (
    <span className="font-monospace">
      {link ? (
        <Copyable text={slot.toString()}>
          <Link href={createClusterPath(`/block/${slot}`)}>
            {slot.toLocaleString("en-US")}
          </Link>
        </Copyable>
      ) : (
        slot.toLocaleString("en-US")
      )}
    </span>
  );
}
