import { PublicKey } from "@solana/web3.js";
import { useEffect, useState } from "react";

export interface AnchorBuild {
  aborted: boolean;
  address: string;
  created_at: string;
  updated_at: string;
  descriptor: string[];
  docker: string;
  id: number;
  name: string;
  sha256: string;
  upgrade_authority: string;
  verified: string;
  verified_slot: number;
  state: string;
}

export function buildLinkPath(verifiedBuild: AnchorBuild) {
  return `https://anchor.projectserum.com/build/${verifiedBuild.id}`;
}

/**
 * Returns a verified build from the anchor registry. null if no such
 * verified build exists, e.g., if the program has been upgraded since the
 * last verified build.
 */
export async function getVerifiedBuild(
  programId: PublicKey,
  limit: number = 5
): Promise<AnchorBuild | null> {
  const url = `https://anchor.projectserum.com/api/v0/program/${programId.toString()}/latest?limit=${limit}`;
  const latestBuildsResp = await fetch(url);

  // Filter out all non successful builds.
  const latestBuilds = (await latestBuildsResp.json()).filter(
    (b: AnchorBuild) =>
      !b.aborted && b.state === "Built" && b.verified === "Verified"
  ) as AnchorBuild[];

  if (latestBuilds.length === 0) {
    return null;
  }

  // Get the latest build.
  return latestBuilds[0];
}

export function useVerifiedBuild(programAddress: PublicKey) {
  const [loading, setLoading] = useState(true);
  const [verifiedBuild, setVerifiedBuild] = useState<AnchorBuild | null>(null);

  useEffect(() => {
    setLoading(true);
    getVerifiedBuild(programAddress)
      .then(setVerifiedBuild)
      .catch((error) => {
        console.log(error);
        setVerifiedBuild(null);
      })
      .finally(() => setLoading(false));
  }, [programAddress, setVerifiedBuild, setLoading]);

  return {
    loading,
    verifiedBuild,
  };
}
