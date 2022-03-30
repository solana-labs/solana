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
const REQUIRED_KEYS: (keyof SecurityTXT)[] = [
  "name",
  "project_url",
  "contacts",
  "policy",
];
const VALID_KEYS: (keyof SecurityTXT)[] = [
  "name",
  "project_url",
  "contacts",
  "policy",
  "preferred_languages",
  "source_code",
  "encryption",
  "auditors",
  "acknowledgements",
  "expiry",
];

const HEADER = "=======BEGIN SECURITY.TXT V1=======\0";
const FOOTER = "=======END SECURITY.TXT V1=======\0";

export const fromProgramData = (
  programData: ProgramDataAccountInfo
): SecurityTXT | undefined => {
  const [data, encoding] = programData.data;

  if (!(data && encoding === "base64")) return undefined;

  const decoded = Buffer.from(data, encoding);

  const header_idx = decoded.indexOf(HEADER);
  const footer_idx = decoded.indexOf(FOOTER);

  if (header_idx < 0 || footer_idx < 0) {
    return undefined;
  }

  /*
  the expected structure of content should be a list
  of ascii encoded key value pairs seperated by null characters.
  e.g. key1\0value1\0key2\0value2\0
  */
  const content = decoded.subarray(header_idx + HEADER.length, footer_idx);

  const map = content
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
    .reduce<{ map: { [key: string]: string }; key: string | undefined }>(
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
              ...(VALID_KEYS.some((x) => x === key) ? { [key]: current } : {}),
              ...prev.map,
            },
            key: undefined,
          };
        }
      },
      { map: {}, key: undefined }
    ).map;
  if (!REQUIRED_KEYS.every((k) => k in map)) {
    throw new Error(`some required fields (${REQUIRED_KEYS}) are missing`);
  }
  return map as SecurityTXT;
};
