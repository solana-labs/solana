import React from "react";
import bs58 from "bs58";
import * as nacl from "tweetnacl";
import { Message, PublicKey } from "@solana/web3.js";
import { Signature } from "components/common/Signature";
import { Address } from "components/common/Address";

export function TransactionSignatures({
  signatures,
  message,
  rawMessage,
}: {
  signatures: (string | null)[];
  message: Message;
  rawMessage: Uint8Array;
}) {
  const signatureRows = React.useMemo(() => {
    return signatures.map((signature, index) => {
      const publicKey = message.accountKeys[index];

      let verified;
      if (signature) {
        const key = publicKey.toBytes();
        const rawSignature = bs58.decode(signature);
        verified = verifySignature({
          message: rawMessage,
          signature: rawSignature,
          key,
        });
      }

      const props = {
        index,
        signature,
        signer: publicKey,
        verified,
      };

      return <SignatureRow key={publicKey.toBase58()} {...props} />;
    });
  }, [signatures, message, rawMessage]);

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title">Signatures</h3>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">#</th>
              <th className="text-muted">Signature</th>
              <th className="text-muted">Signer</th>
              <th className="text-muted">Validity</th>
              <th className="text-muted">Details</th>
            </tr>
          </thead>
          <tbody className="list">{signatureRows}</tbody>
        </table>
      </div>
    </div>
  );
}

function verifySignature({
  message,
  signature,
  key,
}: {
  message: Uint8Array;
  signature: Uint8Array;
  key: Uint8Array;
}): boolean {
  return nacl.sign.detached.verify(message, signature, key);
}

function SignatureRow({
  signature,
  signer,
  verified,
  index,
}: {
  signature: string | null;
  signer: PublicKey;
  verified?: boolean;
  index: number;
}) {
  return (
    <tr>
      <td>
        <span className="badge bg-info-soft me-1">{index + 1}</span>
      </td>
      <td>
        {signature ? (
          <Signature signature={signature} truncateChars={40} />
        ) : (
          "Missing Signature"
        )}
      </td>
      <td>
        <Address pubkey={signer} link />
      </td>
      <td>
        {verified === undefined ? (
          "N/A"
        ) : verified ? (
          <span className="badge bg-success-soft me-1">Valid</span>
        ) : (
          <span className="badge bg-warning-soft me-1">Invalid</span>
        )}
      </td>
      <td>
        {index === 0 && (
          <span className="badge bg-info-soft me-1">Fee Payer</span>
        )}
      </td>
    </tr>
  );
}
