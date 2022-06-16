import React from "react";
import Link from "next/link";
import { useRouter } from "next/router";
import { clusterPath } from "src/utils/url";
import { Copyable } from "./Copyable";

type Props = {
  epoch: number;
  link?: boolean;
};
export function Epoch({ epoch, link }: Props) {
  const router = useRouter();

  return (
    <span className="font-monospace">
      {link ? (
        <Copyable text={epoch.toString()}>
          <Link href={clusterPath(`/epoch/${epoch}`, router.asPath)}>
            {epoch.toLocaleString("en-US")}
          </Link>
        </Copyable>
      ) : (
        epoch.toLocaleString("en-US")
      )}
    </span>
  );
}
