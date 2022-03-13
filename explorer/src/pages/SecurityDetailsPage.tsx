import { PublicKey } from "@solana/web3.js";
import { ErrorCard } from "components/common/ErrorCard";
import { LoadingCard } from "components/common/LoadingCard";
import { TableCardBody } from "components/common/TableCardBody";
import {
  Account,
  useAccountInfo,
  useFetchAccountInfo,
} from "providers/accounts";
import { CacheEntry, FetchStatus } from "providers/cache";
import { ClusterStatus, useCluster } from "providers/cluster";
import React from "react";
import { fromProgramData, SecurityTXT } from "utils/security-txt";

type Props = { address: string };
export function SecurityDetailsPage({ address }: Props) {
  const fetchAccount = useFetchAccountInfo();
  const { status } = useCluster();
  const info = useAccountInfo(address);
  let pubkey: PublicKey | undefined;

  try {
    pubkey = new PublicKey(address);
  } catch (err) {}

  // Fetch account on load
  React.useEffect(() => {
    if (!info && status === ClusterStatus.Connected && pubkey) {
      fetchAccount(pubkey);
    }
  }, [address, status]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="container mt-n3">
      <div className="header">
        <div className="header-body">
          <h6 className="header-pretitle">Details</h6>
          <h2 className="header-title">Security</h2>
          <small>
            Note that this is self-reported by the author of the program and
            might not be acurate.
          </small>
        </div>
      </div>
      {!pubkey ? (
        <ErrorCard text={`Address "${address}" is not valid`} />
      ) : (
        <SecurityDetails pubkey={pubkey} info={info} />
      )}
    </div>
  );
}

enum DisplayType {
  String,
  URL,
  Date,
  Contacts,
  PGP,
  Auditors,
}
type TableRow = {
  display: string;
  key: keyof SecurityTXT;
  type: DisplayType;
};

const ROWS: TableRow[] = [
  {
    display: "Name",
    key: "name",
    type: DisplayType.String,
  },
  {
    display: "Project URL",
    key: "project_url",
    type: DisplayType.URL,
  },
  {
    display: "Contacts",
    key: "contacts",
    type: DisplayType.Contacts,
  },
  {
    display: "Policy",
    key: "policy",
    type: DisplayType.URL,
  },
  {
    display: "Preferred Languages",
    key: "preferred_languages",
    type: DisplayType.String,
  },
  {
    display: "Source Code URL",
    key: "source_code",
    type: DisplayType.URL,
  },
  {
    display: "Encryption",
    key: "encryption",
    type: DisplayType.PGP,
  },
  {
    display: "Auditors",
    key: "auditors",
    type: DisplayType.Auditors,
  },
  {
    display: "Acknowledgements",
    key: "acknowledgements",
    type: DisplayType.URL,
  },
  {
    display: "Expiry",
    key: "expiry",
    type: DisplayType.Date,
  },
];

function SecurityDetails({
  pubkey,
  info,
}: {
  pubkey: PublicKey;
  info?: CacheEntry<Account>;
}) {
  const fetchAccount = useFetchAccountInfo();

  if (!info || info.status === FetchStatus.Fetching) {
    return <LoadingCard />;
  } else if (
    info.status === FetchStatus.FetchFailed ||
    info.data?.lamports === undefined
  ) {
    return <ErrorCard retry={() => fetchAccount(pubkey)} text="Fetch Failed" />;
  }
  const account = info.data;

  const data = account?.details?.data;
  if (!data || data.program !== "bpf-upgradeable-loader" || !data.programData) {
    return <ErrorCard text="Account is not a program" />;
  }

  const securityTXT = fromProgramData(data.programData);

  if (!securityTXT) {
    return <ErrorCard text="Account has no security.txt" />;
  }

  return (
    <div className="card security-txt">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Overview
        </h3>
      </div>
      <TableCardBody>
        {ROWS.filter((x) => x.key in securityTXT).map((x, idx) => {
          return (
            <tr key={idx}>
              <td className="w-100">{x.display}</td>
              <RenderEntry value={securityTXT[x.key]} type={x.type} />
            </tr>
          );
        })}
      </TableCardBody>
    </div>
  );
}

function RenderEntry({
  value,
  type,
}: {
  value: SecurityTXT[keyof SecurityTXT];
  type: DisplayType;
}) {
  if (!value) {
    return <></>;
  }
  switch (type) {
    case DisplayType.String:
      return <td className="text-lg-end font-monospace">{value}</td>;
    case DisplayType.Contacts:
      return (
        <td className="text-lg-end font-monospace">
          <ul>
            {value?.split(",").map((c, i) => {
              const idx = c.indexOf(":");
              const [type, information] = [c.slice(0, idx), c.slice(idx + 1)];
              return (
                <li key={i}>
                  <Contact type={type} information={information} />
                </li>
              );
            })}
          </ul>
        </td>
      );
    case DisplayType.URL:
      if (isValidLink(value)) {
        return (
          <td className="text-lg-end">
            <span className="font-monospace">
              <a href={value}>
                {value}
                <span className="fe fe-external-link ms-2"></span>
              </a>
            </span>
          </td>
        );
      }
      return (
        <td className="text-lg-end">
          <p>{value}</p>
        </td>
      );
    case DisplayType.Date:
      return <td className="text-lg-end font-monospace">{value}</td>;
    case DisplayType.PGP:
      if (isValidLink(value)) {
        return (
          <td className="text-lg-end">
            <span className="font-monospace">
              <a href={value}>
                {value}
                <span className="fe fe-external-link ms-2"></span>
              </a>
            </span>
          </td>
        );
      }
      return (
        <td>
          <code>{value.trim()}</code>
        </td>
      );
    case DisplayType.Auditors:
      if (isValidLink(value)) {
        return (
          <td className="text-lg-end">
            <span className="font-monospace">
              <a href={value}>
                {value}
                <span className="fe fe-external-link ms-2"></span>
              </a>
            </span>
          </td>
        );
      }
      return (
        <td>
          <ul>
            {value?.split(",").map((c, idx) => {
              return <li key={idx}>{c}</li>;
            })}
          </ul>
        </td>
      );
    default:
      break;
  }
  return <></>;
}

function isValidLink(value: string) {
  try {
    const url = new URL(value);
    return ["http:", "https:"].includes(url.protocol);
  } catch (err) {
    return false;
  }
}

function Contact({ type, information }: { type: string; information: string }) {
  switch (type) {
    case "discord":
      return (
        <a href={`https://discordapp.com/users/${information}`}>
          Discord: {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "email":
      return (
        <a href={`mailto:${information}`}>
          {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "telegram":
      return (
        <a href={`https://t.me/${information}`}>
          Telegram: {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "twitter":
      return (
        <a href={`https://twitter.com/${information}`}>
          Twitter {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "link":
      if (isValidLink(information)) {
        return (
          <a href={`${information}`}>
            {information}
            <span className="fe fe-external-link ms-2"></span>
          </a>
        );
      }
      return <>{information}</>;
    case "other":
    default:
      return (
        <>
          {type}: {information}
        </>
      );
  }
}
