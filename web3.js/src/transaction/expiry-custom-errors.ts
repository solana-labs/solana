export class TransactionExpiredBlockheightExceededError extends Error {
  signature: string;

  constructor(signature: string) {
    super(`Signature ${signature} has expired: block height exceeded.`);
    this.signature = signature;
  }
}

Object.defineProperty(
  TransactionExpiredBlockheightExceededError.prototype,
  'name',
  {
    value: 'TransactionExpiredBlockheightExceededError',
  },
);

export class TransactionExpiredTimeoutError extends Error {
  signature: string;

  constructor(signature: string, timeoutSeconds: number) {
    super(
      `Transaction was not confirmed in ${timeoutSeconds.toFixed(
        2,
      )} seconds. It is ` +
        'unknown if it succeeded or failed. Check signature ' +
        `${signature} using the Solana Explorer or CLI tools.`,
    );
    this.signature = signature;
  }
}

Object.defineProperty(TransactionExpiredTimeoutError.prototype, 'name', {
  value: 'TransactionExpiredTimeoutError',
});

export class TransactionExpiredNonceInvalidError extends Error {
  signature: string;

  constructor(signature: string) {
    super(`Signature ${signature} has expired: the nonce is no longer valid.`);
    this.signature = signature;
  }
}

Object.defineProperty(TransactionExpiredNonceInvalidError.prototype, 'name', {
  value: 'TransactionExpiredNonceInvalidError',
});
