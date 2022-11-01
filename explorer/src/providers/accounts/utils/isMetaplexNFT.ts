import { MintAccountInfo } from "validators/accounts/token";
import { ParsedData } from "..";

export default function isMetaplexNFT(
  parsedData?: ParsedData,
  mintInfo?: MintAccountInfo
) {
  return (
    parsedData?.program === "spl-token" &&
    parsedData?.parsed.type === "mint" &&
    parsedData?.nftData &&
    mintInfo?.decimals === 0 &&
    (parseInt(mintInfo.supply) === 1 ||
      parsedData?.nftData?.metadata?.tokenStandard === 1)
  );
}
