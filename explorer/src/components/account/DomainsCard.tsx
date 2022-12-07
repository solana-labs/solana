import React from "react";
import { PublicKey } from "@solana/web3.js";
import { useUserDomains, DomainInfo } from "../../utils/name-service";
import { useUserANSDomains } from "../../utils/ans-domains";
import { LoadingCard } from "components/common/LoadingCard";
import { ErrorCard } from "components/common/ErrorCard";
import { Address } from "components/common/Address";

export function DomainsCard({ pubkey }: { pubkey: PublicKey }) {
  const [domains, domainsLoading] = useUserDomains(pubkey);
  const [domainsANS, domainsANSLoading] = useUserANSDomains(pubkey);


  if ((domainsLoading && (!domains || domains.length === 0)) || (domainsANSLoading && (!domainsANS || domainsANS.length === 0))) {
    return <LoadingCard message="Loading domains" />;
  } else if (!domains || !domainsANS) {
    return <ErrorCard text="Failed to fetch domains" />;
  }

  if (domains.length === 0 && domainsANS.length === 0) {
    return <ErrorCard text="No domain name found" />;
  }
  let allDomains = domains;
  if (domainsANS) {
    allDomains = [...allDomains, ...domainsANS]
  }
  allDomains.sort((a, b) => a.name.localeCompare(b.name));

  return (
    <div className="card">
      <div className="card-header align-items-center">
        <h3 className="card-header-title">Owned Domain Names</h3>
      </div>
      <div className="table-responsive mb-0">
        <table className="table table-sm table-nowrap card-table">
          <thead>
            <tr>
              <th className="text-muted">Domain Name</th>
              <th className="text-muted">Name Service Account</th>
            </tr>
          </thead>
          <tbody className="list">
            {allDomains.map((domain) => (
              <RenderDomainRow
                key={domain.address.toBase58()}
                domainInfo={domain}
              />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function RenderDomainRow({ domainInfo }: { domainInfo: DomainInfo }) {
  return (
    <tr>
      <td>{domainInfo.name}</td>
      <td>
        <Address pubkey={domainInfo.address} link />
      </td>
    </tr>
  );
}
