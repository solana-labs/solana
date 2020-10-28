/* eslint-disable @typescript-eslint/no-redeclare */

import { object, any, StructType, string } from "superstruct";

export type ParsedInfo = StructType<typeof ParsedInfo>;
export const ParsedInfo = object({
  type: string(),
  info: any(),
});
