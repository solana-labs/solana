// @flow

/**
 * @typedef {Object} FeeCalculator
 * @property {number} lamportsPerSignature lamports Cost in lamports to validate a signature
 */
export type FeeCalculator = {
  lamportsPerSignature: number,
};
