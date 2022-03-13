import { PublicKey } from "@solana/web3.js";
import { SecurityTXT } from "utils/security-txt";

export function SecurityTXTBadge({
  securityTXT,
  pubkey,
}: {
  securityTXT: SecurityTXT | undefined;
  pubkey: PublicKey;
}) {
  //TODO: href params
  if (securityTXT) {
    return (
      <h3 className="mb-0">
        <a
          className="c-pointer badge bg-success-soft rank"
          href={`/security/${pubkey.toBase58()}${window.location.search}`}
          target="_blank"
          rel="noreferrer"
        >
          Included
        </a>
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
