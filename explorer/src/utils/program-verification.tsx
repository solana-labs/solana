import { PublicKey } from "@solana/web3.js";
import { useEffect, useState } from "react";

export type VerifiableBuild =
  | {
      label: string;
      id: number;
      verified_slot: number;
      url: string;
    }
  | {
      label: string;
      verified_slot: null;
    };

export function useVerifiableBuilds(programAddress: PublicKey) {
  const { loading: loadingAnchor, verifiableBuild: verifiedBuildAnchor } =
    useAnchorVerifiableBuild(programAddress);

  return {
    loading: loadingAnchor,
    verifiableBuilds: [verifiedBuildAnchor],
  };
}

// ANCHOR

const defaultAnchorBuild = {
  label: "Anchor",
  verified_slot: null,
};

export function useAnchorVerifiableBuild(programAddress: PublicKey) {
  const [loading, setLoading] = useState(true);
  const [verifiableBuild, setVerifiableBuild] =
    useState<VerifiableBuild>(defaultAnchorBuild);

  useEffect(() => {
    setLoading(true);
    getAnchorVerifiableBuild(programAddress)
      .then(setVerifiableBuild)
      .catch((error) => {
        console.log(error);
        setVerifiableBuild(defaultAnchorBuild);
      })
      .finally(() => setLoading(false));
  }, [programAddress, setVerifiableBuild, setLoading]);

  return {
    loading,
    verifiableBuild,
  };
}

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

/**
 * Returns a verified build from the anchor registry. null if no such
 * verified build exists, e.g., if the program has been upgraded since the
 * last verified build.
 */
export async function getAnchorVerifiableBuild(
  programId: PublicKey,
  limit: number = 5
): Promise<VerifiableBuild> {
  const url = `https://anchor.projectserum.com/api/v0/program/${programId.toString()}/latest?limit=${limit}`;
  const latestBuildsResp = await fetch(url);

  // Filter out all non successful builds.
  const latestBuilds = (await latestBuildsResp.json()).filter(
    (b: AnchorBuild) =>
      !b.aborted && b.state === "Built" && b.verified === "Verified"
  ) as AnchorBuild[];

  if (latestBuilds.length === 0) {
    return defaultAnchorBuild;
  }

  // Get the latest build.
  const { verified_slot, id } = latestBuilds[0];
  return {
    ...defaultAnchorBuild,
    verified_slot,
    id,
    url: `https://anchor.projectserum.com/build/${id}`,
  };
}

// END ANCHOR
