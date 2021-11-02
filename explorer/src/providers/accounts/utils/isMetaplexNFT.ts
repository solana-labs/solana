import { ProgramData } from "..";

export default function isMetaplexNFT(data?: ProgramData, decimals?: number) {
  return (
    data?.program === "spl-token" &&
    data?.parsed.type === "mint" &&
    data?.nftData &&
    decimals === 0
  );
}
