import React from "react";
import Link from "next/link";
import { useRouter } from "next/router";
import { clusterPath } from "src/utils/url";
import { Copyable } from "./Copyable";

type Props = {
  slot: number;
  link?: boolean;
};
export function Slot({ slot, link }: Props) {
  const router = useRouter();

  return (
    <span className="font-monospace">
      {link ? (
        <Copyable text={slot.toString()}>
          <Link href={clusterPath(`/block/${slot}`, router.asPath)}>
            {slot.toLocaleString("en-US")}
          </Link>
        </Copyable>
      ) : (
        slot.toLocaleString("en-US")
      )}
    </span>
  );
}
