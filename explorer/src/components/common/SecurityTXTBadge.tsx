import { PublicKey } from "@solana/web3.js";
import { Link } from "react-router-dom";
import { SecurityTXT } from "utils/security-txt";
import { clusterPath } from "utils/url";

export function SecurityTXTBadge({
  securityTXT,
  pubkey,
}: {
  securityTXT: SecurityTXT | undefined;
  pubkey: PublicKey;
}) {
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
        <span className="badge bg-warning-soft rank">Not included</span>
      </h3>
    );
  }
}
