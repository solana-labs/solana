import {expect} from 'chai';

import {EpochSchedule} from '../src';

describe('EpochSchedule', () => {
  it('slot methods work', () => {
    const firstNormalEpoch = 8;
    const firstNormalSlot = 8160;
    const leaderScheduleSlotOffset = 8192;
    const slotsPerEpoch = 8192;
    const warmup = false;

    const epochSchedule = new EpochSchedule(
      slotsPerEpoch,
      leaderScheduleSlotOffset,
      warmup,
      firstNormalEpoch,
      firstNormalSlot,
    );

    expect(epochSchedule.getSlotsInEpoch(4)).to.be.equal(512);
    expect(epochSchedule.getSlotsInEpoch(100)).to.be.equal(8192);

    expect(epochSchedule.getFirstSlotInEpoch(2)).to.be.equal(96);
    expect(epochSchedule.getLastSlotInEpoch(2)).to.be.equal(223);

    expect(epochSchedule.getFirstSlotInEpoch(10)).to.be.equal(8160 + 2 * 8192);
    expect(epochSchedule.getLastSlotInEpoch(10)).to.be.equal(
      8160 + 3 * 8192 - 1,
    );
  });
});
