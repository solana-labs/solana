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

  constructor(signature: string) {
    super(`Signature ${signature} has expired: timeout exceeded.`);
    this.signature = signature;
  }
}

Object.defineProperty(TransactionExpiredTimeoutError.prototype, 'name', {
  value: 'TransactionExpiredTimeoutError',
});
