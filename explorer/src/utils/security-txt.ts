import { ProgramDataAccountInfo } from "validators/accounts/upgradeable-program";

export type SecurityTXT = {
  name: string;
  project_url: string;
  contacts: string;
  policy: string;
  preferred_languages?: string;
  source_code?: string;
  encryption?: string;
  auditors?: string;
  acknowledgements?: string;
  expiry?: string;
};

const HEADER = "=======BEGIN SECURITY.TXT V1=======\0";
const FOOTER = "=======END SECURITY.TXT V1=======\0";

export const fromProgramData = (
  programData: ProgramDataAccountInfo
): SecurityTXT | undefined => {
  const [data, encoding] = programData.data;

  //TODO: check if other encoding exists
  if (!(data && encoding === "base64")) return undefined;

  const decoded = Buffer.from(data, encoding);

  const header_idx = decoded.indexOf(HEADER);
  const footer_idx = decoded.indexOf(FOOTER);

  if (header_idx < 0 || footer_idx < 0) {
    return undefined;
  }

  const content = decoded.subarray(header_idx + HEADER.length, footer_idx);

  return content
    .reduce<number[][]>(
      (prev, current) => {
        if (current === 0) {
          prev.push([]);
        } else {
          prev[prev.length - 1].push(current);
        }
        return prev;
      },
      [[]]
    )
    .map((c) => String.fromCharCode(...c))
    .reduce<{ map: {}; key: string | undefined }>(
      (prev, current) => {
        const key = prev.key;
        if (!key) {
          return {
            map: prev.map,
            key: current,
          };
        } else {
          return {
            map: {
              [key]: current,
              ...prev.map,
            },
            key: undefined,
          };
        }
      },
      { map: {}, key: undefined }
    ).map as SecurityTXT;
};
