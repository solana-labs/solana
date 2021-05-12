import BN from 'bn.js';
import {Buffer} from 'buffer';
import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {Keypair} from '../src/keypair';
import {PublicKey, MAX_SEED_LENGTH} from '../src/publickey';

use(chaiAsPromised);

describe('PublicKey', function () {
  it('invalid', () => {
    expect(() => {
      new PublicKey([
        3,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
      ]);
    }).to.throw();

    expect(() => {
      new PublicKey(
        '0x300000000000000000000000000000000000000000000000000000000000000000000',
      );
    }).to.throw();

    expect(() => {
      new PublicKey(
        '0x300000000000000000000000000000000000000000000000000000000000000',
      );
    }).to.throw();

    expect(() => {
      new PublicKey(
        '135693854574979916511997248057056142015550763280047535983739356259273198796800000',
      );
    }).to.throw();

    expect(() => {
      new PublicKey('12345');
    }).to.throw();
  });

  it('equals', () => {
    const arrayKey = new PublicKey([
      3,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
    ]);
    const base58Key = new PublicKey(
      'CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3',
    );

    expect(arrayKey.equals(base58Key)).to.be.true;
  });

  it('toBase58', () => {
    const key = new PublicKey('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
    expect(key.toBase58()).to.eq('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
    expect(key.toString()).to.eq('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

    const key2 = new PublicKey('1111111111111111111111111111BukQL');
    expect(key2.toBase58()).to.eq('1111111111111111111111111111BukQL');
    expect(key2.toString()).to.eq('1111111111111111111111111111BukQL');

    const key3 = new PublicKey('11111111111111111111111111111111');
    expect(key3.toBase58()).to.eq('11111111111111111111111111111111');

    const key4 = new PublicKey([
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
    ]);
    expect(key4.toBase58()).to.eq('11111111111111111111111111111111');
  });

  it('toBuffer', () => {
    const key = new PublicKey('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
    expect(key.toBuffer()).to.have.length(32);
    expect(key.toBase58()).to.eq('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

    const key2 = new PublicKey('11111111111111111111111111111111');
    expect(key2.toBuffer()).to.have.length(32);
    expect(key2.toBase58()).to.eq('11111111111111111111111111111111');

    const key3 = new PublicKey(0);
    expect(key3.toBuffer()).to.have.length(32);
    expect(key3.toBase58()).to.eq('11111111111111111111111111111111');
  });

  it('equals (II)', () => {
    const key1 = new PublicKey([
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      1,
    ]);
    const key2 = new PublicKey(key1.toBuffer());

    expect(key1.equals(key2)).to.be.true;
  });

  it('createWithSeed', async () => {
    const defaultPublicKey = new PublicKey('11111111111111111111111111111111');
    const derivedKey = await PublicKey.createWithSeed(
      defaultPublicKey,
      'limber chicken: 4/45',
      defaultPublicKey,
    );

    expect(
      derivedKey.equals(
        new PublicKey('9h1HyLCW5dZnBVap8C5egQ9Z6pHyjsh5MNy83iPqqRuq'),
      ),
    ).to.be.true;
  });

  it('createProgramAddress', async () => {
    const programId = new PublicKey(
      'BPFLoader1111111111111111111111111111111111',
    );
    const publicKey = new PublicKey(
      'SeedPubey1111111111111111111111111111111111',
    );

    let programAddress = await PublicKey.createProgramAddress(
      [Buffer.from('', 'utf8'), Buffer.from([1])],
      programId,
    );
    expect(
      programAddress.equals(
        new PublicKey('3gF2KMe9KiC6FNVBmfg9i267aMPvK37FewCip4eGBFcT'),
      ),
    ).to.be.true;

    programAddress = await PublicKey.createProgramAddress(
      [Buffer.from('☉', 'utf8')],
      programId,
    );
    expect(
      programAddress.equals(
        new PublicKey('7ytmC1nT1xY4RfxCV2ZgyA7UakC93do5ZdyhdF3EtPj7'),
      ),
    ).to.be.true;

    programAddress = await PublicKey.createProgramAddress(
      [Buffer.from('Talking', 'utf8'), Buffer.from('Squirrels', 'utf8')],
      programId,
    );
    expect(
      programAddress.equals(
        new PublicKey('HwRVBufQ4haG5XSgpspwKtNd3PC9GM9m1196uJW36vds'),
      ),
    ).to.be.true;

    programAddress = await PublicKey.createProgramAddress(
      [publicKey.toBuffer()],
      programId,
    );
    expect(
      programAddress.equals(
        new PublicKey('GUs5qLUfsEHkcMB9T38vjr18ypEhRuNWiePW2LoK4E3K'),
      ),
    ).to.be.true;

    const programAddress2 = await PublicKey.createProgramAddress(
      [Buffer.from('Talking', 'utf8')],
      programId,
    );
    expect(programAddress.equals(programAddress2)).to.eq(false);

    await expect(
      PublicKey.createProgramAddress(
        [Buffer.alloc(MAX_SEED_LENGTH + 1)],
        programId,
      ),
    ).to.be.rejectedWith('Max seed length exceeded');

    // https://github.com/solana-labs/solana/issues/11950
    {
      let seeds = [
        new PublicKey(
          'H4snTKK9adiU15gP22ErfZYtro3aqR9BTMXiH3AwiUTQ',
        ).toBuffer(),
        new BN(2).toArrayLike(Buffer, 'le', 8),
      ];
      let programId = new PublicKey(
        '4ckmDgGdxQoPDLUkDT3vHgSAkzA3QRdNq5ywwY4sUSJn',
      );
      programAddress = await PublicKey.createProgramAddress(seeds, programId);
      expect(
        programAddress.equals(
          new PublicKey('12rqwuEgBYiGhBrDJStCiqEtzQpTTiZbh7teNVLuYcFA'),
        ),
      ).to.be.true;
    }
  });

  it('findProgramAddress', async () => {
    const programId = new PublicKey(
      'BPFLoader1111111111111111111111111111111111',
    );
    let [programAddress, nonce] = await PublicKey.findProgramAddress(
      [Buffer.from('', 'utf8')],
      programId,
    );
    expect(
      programAddress.equals(
        await PublicKey.createProgramAddress(
          [Buffer.from('', 'utf8'), Buffer.from([nonce])],
          programId,
        ),
      ),
    ).to.be.true;
  });

  it('isOnCurve', () => {
    let onCurve = Keypair.generate().publicKey;
    expect(PublicKey.isOnCurve(onCurve.toBuffer())).to.be.true;
    // A program address, yanked from one of the above tests. This is a pretty
    // poor test vector since it was created by the same code it is testing.
    // Unfortunately, I've been unable to find a golden negative example input
    // for curve25519 point decompression :/
    let offCurve = new PublicKey(
      '12rqwuEgBYiGhBrDJStCiqEtzQpTTiZbh7teNVLuYcFA',
    );
    expect(PublicKey.isOnCurve(offCurve.toBuffer())).to.be.false;
  });
});
