/* eslint-disable @typescript-eslint/no-redeclare */

import { type, any, Infer, string } from "superstruct";

export type ParsedInfo = Infer<typeof ParsedInfo>;
export const ParsedInfo = type({
  type: string(),
  info: any(),
});
