import React from "react";
import Link from "next/link";
import { useCreateClusterPath } from "utils/routing";
import { Copyable } from "./Copyable";

type Props = {
  epoch: number | bigint;
  link?: boolean;
};
export function Epoch({ epoch, link }: Props) {
  const createClusterPath = useCreateClusterPath();

  return (
    <span className="font-monospace">
      {link ? (
        <Copyable text={epoch.toString()}>
          <Link href={createClusterPath(`/epoch/${epoch}`)}>
            {epoch.toLocaleString("en-US")}
          </Link>
        </Copyable>
      ) : (
        epoch.toLocaleString("en-US")
      )}
    </span>
  );
}
