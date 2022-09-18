import { Address } from "components/common/Address";
import { TableCardBody } from "components/common/TableCardBody";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { Suspense } from "react";
import { UnknownAccountCard } from "../UnknownAccountCard";
import { useParseGlowID } from "./useParseGlowID";

export function GlowIDAccountSection({ account }: { account: Account }) {
  return (
    <Suspense fallback={<div>Loading...</div>}>
      <GlowIdCard account={account} />
    </Suspense>
  );
}

const GlowIdCard = ({ account }: { account: Account }) => {
  const fetchInfo = useFetchAccountInfo();
  const refresh = () => fetchInfo(account.pubkey);
  const { data: glowId } = useParseGlowID(account);

  if (!glowId) {
    return <UnknownAccountCard account={account} />;
  }

  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Overview
        </h3>
        <button className="btn btn-white btn-sm" onClick={refresh}>
          <span className="fe fe-refresh-cw me-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Handle Address</td>
          <td className="text-lg-end">
            <Address pubkey={glowId.address} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Wallet Address</td>
          <td className="text-lg-end">
            <Address pubkey={glowId.resolved} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Joined At</td>
          <td className="text-lg-end">
            {Intl.DateTimeFormat(undefined, { dateStyle: "full" }).format(
              glowId.joined_at
            )}
          </td>
        </tr>
        <tr>
          <td>Twitter</td>
          <td className="text-lg-end">
            {glowId.twitter ? (
              <a
                href={`https://twitter.com/${glowId.twitter}`}
                target={"_blank"}
                rel="noreferrer noopener"
              >
                {glowId.twitter}
              </a>
            ) : (
              <span>Not Linked</span>
            )}
          </td>
        </tr>
        <tr>
          <td>Website</td>
          <td className="text-lg-end">
            <a
              href={`https://glow.xyz/${glowId.handle}`}
              target={"_blank"}
              rel="noreferrer noopener"
            >
              glow.xyz/{glowId.handle}
            </a>
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
};
