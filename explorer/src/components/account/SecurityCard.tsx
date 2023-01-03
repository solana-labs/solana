import { ErrorCard } from "components/common/ErrorCard";
import { TableCardBody } from "components/common/TableCardBody";
import { UpgradeableLoaderAccountData } from "providers/accounts";
import { fromProgramData, SecurityTXT } from "utils/security-txt";

export function SecurityCard({ data }: { data: UpgradeableLoaderAccountData }) {
  if (!data.programData) {
    return <ErrorCard text="Account has no data" />;
  }

  const { securityTXT, error } = fromProgramData(data.programData);
  if (!securityTXT) {
    return <ErrorCard text={error!} />;
  }

  return (
    <div className="card security-txt">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Security.txt
        </h3>
        <small>
          Note that this is self-reported by the author of the program and might
          not be accurate.
        </small>
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
    display: "Secure Contact Encryption",
    key: "encryption",
    type: DisplayType.PGP,
  },
  {
    display: "Source Code URL",
    key: "source_code",
    type: DisplayType.URL,
  },
  {
    display: "Source Code Release Version",
    key: "source_release",
    type: DisplayType.String,
  },
  {
    display: "Source Code Revision",
    key: "source_revision",
    type: DisplayType.String,
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
              if (idx < 0) {
                //invalid contact
                return <li key={i}>{c}</li>;
              }
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
              <a rel="noopener noreferrer" target="_blank" href={value}>
                {value}
                <span className="fe fe-external-link ms-2"></span>
              </a>
            </span>
          </td>
        );
      }
      return (
        <td className="text-lg-end">
          <pre>{value.trim()}</pre>
        </td>
      );
    case DisplayType.Date:
      return <td className="text-lg-end font-monospace">{value}</td>;
    case DisplayType.PGP:
      if (isValidLink(value)) {
        return (
          <td className="text-lg-end">
            <span className="font-monospace">
              <a rel="noopener noreferrer" target="_blank" href={value}>
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
              <a rel="noopener noreferrer" target="_blank" href={value}>
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
      return <>Discord: {information}</>;
    case "email":
      return (
        <a
          rel="noopener noreferrer"
          target="_blank"
          href={`mailto:${information}`}
        >
          {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "telegram":
      return (
        <a
          rel="noopener noreferrer"
          target="_blank"
          href={`https://t.me/${information}`}
        >
          Telegram: {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "twitter":
      return (
        <a
          rel="noopener noreferrer"
          target="_blank"
          href={`https://twitter.com/${information}`}
        >
          Twitter {information}
          <span className="fe fe-external-link ms-2"></span>
        </a>
      );
    case "link":
      if (isValidLink(information)) {
        return (
          <a rel="noopener noreferrer" target="_blank" href={`${information}`}>
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
