import { ProgramDataAccountInfo } from "validators/accounts/upgradeable-program";

export type SecurityTXT = {
  name: string;
  project_url: string;
  contacts: string;
  policy: string;
  preferred_languages?: string;
  encryption?: string;
  source_code?: string;
  source_release?: string;
  source_revision?: string;
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
  "encryption",
  "source_code",
  "source_release",
  "source_revision",
  "auditors",
  "acknowledgements",
  "expiry",
];

const HEADER = "=======BEGIN SECURITY.TXT V1=======\0";
const FOOTER = "=======END SECURITY.TXT V1=======\0";

export const fromProgramData = (
  programData: ProgramDataAccountInfo
): { securityTXT?: SecurityTXT; error?: string } => {
  const [data, encoding] = programData.data;
  if (!(data && encoding === "base64"))
    return { securityTXT: undefined, error: "Failed to decode program data" };

  const decoded = Buffer.from(data, encoding);

  const headerIdx = decoded.indexOf(HEADER);
  const footerIdx = decoded.indexOf(FOOTER);

  if (headerIdx < 0 || footerIdx < 0) {
    return { securityTXT: undefined, error: "Program has no security.txt" };
  }

  /*
  the expected structure of content should be a list
  of ascii encoded key value pairs separated by null characters.
  e.g. key1\0value1\0key2\0value2\0
  */
  const content = decoded.subarray(headerIdx + HEADER.length, footerIdx);

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
    return {
      securityTXT: undefined,
      error: `some required fields (${REQUIRED_KEYS}) are missing`,
    };
  }
  return { securityTXT: map as SecurityTXT, error: undefined };
};
