import {expect} from 'chai';
import {Buffer} from 'buffer';

import {Keypair} from '../src';

describe('Keypair', () => {
  it('new keypair', () => {
    const keypair = new Keypair();
    expect(keypair.secretKey).to.have.length(64);
    expect(keypair.publicKey.toBytes()).to.have.length(32);
  });

  it('generate new keypair', () => {
    const keypair = Keypair.generate();
    expect(keypair.secretKey).to.have.length(64);
  });

  it('create keypair from secret key', () => {
    const secretKey = Buffer.from(
      'mdqVWeFekT7pqy5T49+tV12jO0m+ESW7ki4zSU9JiCgbL0kJbj5dvQ/PqcDAzZLZqzshVEs01d1KZdmLh4uZIg==',
      'base64',
    );
    const keypair = Keypair.fromSecretKey(secretKey);
    expect(keypair.publicKey.toBase58()).to.eq(
      '2q7pyhPwAwZ3QMfZrnAbDhnh9mDUqycszcpf86VgQxhF',
    );
    expect(keypair.secretKey.toString()).to.eq(
      '153,218,149,89,225,94,145,62,233,171,46,83,227,223,173,87,93,163,59,73,' +
        '190,17,37,187,146,46,51,73,79,73,136,40,27,47,73,9,110,62,93,189,15,207,' +
        '169,192,192,205,146,217,171,59,33,84,75,52,213,221,74,101,217,139,135,139,153,34',
    );
  });

  it('creating keypair from invalid secret key throws error', () => {
    const secretKey = Buffer.from(
      'mdqVWeFekT7pqy5T49+tV12jO0m+ESW7ki4zSU9JiCgbL0kJbj5dvQ/PqcDAzZLZqzshVEs01d1KZdmLh4uZIG==',
      'base64',
    );
    expect(() => {
      Keypair.fromSecretKey(secretKey);
    }).to.throw('provided secretKey is invalid');
  });

  it('creating keypair from invalid secret key succeeds if validation is skipped', () => {
    const secretKey = Buffer.from(
      'mdqVWeFekT7pqy5T49+tV12jO0m+ESW7ki4zSU9JiCgbL0kJbj5dvQ/PqcDAzZLZqzshVEs01d1KZdmLh4uZIG==',
      'base64',
    );
    const keypair = Keypair.fromSecretKey(secretKey, {skipValidation: true});
    expect(keypair.publicKey.toBase58()).to.eq(
      '2q7pyhPwAwZ3QMfZrnAbDhnh9mDUqycszcpf86VgQxhD',
    );
  });

  it('generate keypair from random seed', () => {
    const keypair = Keypair.fromSeed(Uint8Array.from(Array(32).fill(8)));
    expect(keypair.publicKey.toBase58()).to.eq(
      '2KW2XRd9kwqet15Aha2oK3tYvd3nWbTFH1MBiRAv1BE1',
    );
  });
});
