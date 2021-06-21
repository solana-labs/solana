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
