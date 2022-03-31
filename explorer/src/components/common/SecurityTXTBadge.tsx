import { PublicKey } from "@solana/web3.js";
import { Link } from "react-router-dom";
import { fromProgramData } from "utils/security-txt";
import { clusterPath } from "utils/url";
import { ProgramDataAccountInfo } from "validators/accounts/upgradeable-program";

export function SecurityTXTBadge({
  programData,
  pubkey,
}: {
  programData: ProgramDataAccountInfo;
  pubkey: PublicKey;
}) {
  const { securityTXT, error } = fromProgramData(programData);
  if (securityTXT) {
    return (
      <h3 className="mb-0">
        <Link
          className="c-pointer badge bg-success-soft rank"
          to={clusterPath(`/address/${pubkey.toBase58()}/security`)}
        >
          Included
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
