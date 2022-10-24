import { PublicKey } from "@solana/web3.js";
import Link from "next/link";
import { fromProgramData } from "utils/security-txt";
import { useCreateClusterPath } from "utils/routing";
import { ProgramDataAccountInfo } from "validators/accounts/upgradeable-program";

export function SecurityTXTBadge({
  programData,
  pubkey,
}: {
  programData: ProgramDataAccountInfo;
  pubkey: PublicKey;
}) {
  const createClusterPath = useCreateClusterPath();
  const { securityTXT, error } = fromProgramData(programData);

  if (securityTXT) {
    return (
      <h3 className="mb-0">
        <Link
          href={createClusterPath(`/address/${pubkey.toBase58()}/security`)}
        >
          <a className="c-pointer badge bg-success-soft rank">Included</a>
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
