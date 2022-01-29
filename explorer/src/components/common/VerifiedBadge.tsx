import { PublicKey } from "@solana/web3.js";
import { buildLinkPath, useVerifiedBuild } from "utils/program-verification";

export function VerifiedBadge({
  programAddress,
  programDeploySlot,
}: {
  programAddress: PublicKey;
  programDeploySlot: number;
}) {
  const { loading, verifiedBuild } = useVerifiedBuild(programAddress);
  if (loading)
    return (
      <h3 className="mb-0">
        <span className="badge bg-dark rank">Checking</span>
      </h3>
    );

  if (verifiedBuild && verifiedBuild.verified_slot === programDeploySlot) {
    return (
      <h3 className="mb-0">
        <a
          className="c-pointer badge bg-success-soft rank"
          href={buildLinkPath(verifiedBuild)}
          target="_blank"
          rel="noreferrer"
        >
          Verified
        </a>
      </h3>
    );
  } else {
    return (
      <h3 className="mb-0">
        <span className="badge bg-warning-soft rank">Unverified</span>
      </h3>
    );
  }
}
