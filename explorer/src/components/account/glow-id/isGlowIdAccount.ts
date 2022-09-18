import { GLOW_ID_ADDRESS } from "./glow-id-utils";
import { Account } from "../../../providers/accounts";

export function isGlowIdAccount(account: Account): boolean {
  return Boolean(
    account.details?.owner.toBase58() === GLOW_ID_ADDRESS &&
      account.details.rawData
  );
}
