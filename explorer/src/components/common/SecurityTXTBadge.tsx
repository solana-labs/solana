import { PublicKey } from "@solana/web3.js";
import Link from "next/link";
import { useRouter } from "next/router";
import { fromProgramData } from "src/utils/security-txt";
import { clusterPath } from "src/utils/url";
import { ProgramDataAccountInfo } from "src/validators/accounts/upgradeable-program";

export function SecurityTXTBadge({
  programData,
  pubkey,
}: {
  programData: ProgramDataAccountInfo;
  pubkey: PublicKey;
}) {
  const router = useRouter();
  const { securityTXT, error } = fromProgramData(programData);

  if (securityTXT) {
    return (
      <h3 className="mb-0">
        <Link href={clusterPath(`/address/${pubkey.toBase58()}/security`, router.asPath)}>
          <span className="c-pointer badge bg-success-soft rank">Included</span>
        </Link>
      </h3>
    );
  } else {
    return (
      <h3 className="mb-0">
        <span className="badge bg-warning-soft rank">{error}</span>
      </h3>
    );
  }
}
