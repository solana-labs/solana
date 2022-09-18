import useSWR, { SWRResponse } from "swr";
import { Account } from "../../../providers/accounts";
import { GlowIdTypes, parseGlowIdAccount } from "./glow-id-utils";

export const useParseGlowID = (
  account: Account
): {
  data: GlowIdTypes.Info | null;
  error: any;
  mutate: SWRResponse<GlowIdTypes.Info | null, never>["mutate"];
} => {
  const swrKey = [account.pubkey.toBase58()];
  const { data, error, mutate } = useSWR(
    swrKey,
    async () => {
      return await parseGlowIdAccount({
        data: account.details?.rawData,
        handlePubkey: account.pubkey,
      });
    },
    {
      suspense: true,
    }
  );
  // Not nullable since we use suspense
  return { data: data!, error, mutate };
};
