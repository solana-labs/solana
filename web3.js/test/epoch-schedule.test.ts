import {expect} from 'chai';

import {EpochSchedule} from '../src';

describe('EpochSchedule', () => {
  it('slot methods work', () => {
    const firstNormalEpoch = 14;
    const firstNormalSlot = 524_256;
    const leaderScheduleSlotOffset = 432_000;
    const slotsPerEpoch = 432_000;
    const warmup = true;

    const epochSchedule = new EpochSchedule(
      slotsPerEpoch,
      leaderScheduleSlotOffset,
      warmup,
      firstNormalEpoch,
      firstNormalSlot,
    );

    expect(epochSchedule.getEpoch(35)).to.be.equal(1);
    expect(epochSchedule.getEpochAndSlotIndex(35)).to.be.eql([1, 3]);

    expect(
      epochSchedule.getEpoch(firstNormalSlot + 3 * slotsPerEpoch + 12345),
    ).to.be.equal(17);
    expect(
      epochSchedule.getEpochAndSlotIndex(
        firstNormalSlot + 3 * slotsPerEpoch + 12345,
      ),
    ).to.be.eql([17, 12345]);

    expect(epochSchedule.getSlotsInEpoch(4)).to.be.equal(512);
    expect(epochSchedule.getSlotsInEpoch(100)).to.be.equal(slotsPerEpoch);

    expect(epochSchedule.getFirstSlotInEpoch(2)).to.be.equal(96);
    expect(epochSchedule.getLastSlotInEpoch(2)).to.be.equal(223);

    expect(epochSchedule.getFirstSlotInEpoch(16)).to.be.equal(
      firstNormalSlot + 2 * slotsPerEpoch,
    );
    expect(epochSchedule.getLastSlotInEpoch(16)).to.be.equal(
      firstNormalSlot + 3 * slotsPerEpoch - 1,
    );
  });
});
