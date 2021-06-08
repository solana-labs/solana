const MINIMUM_SLOT_PER_EPOCH = 32;

// Returns the number of trailing zeros in the binary representation of self.
function trailingZeros(n: number) {
  let trailingZeros = 0;
  while ((n & 1) == 0) {
    n /= 2;
    trailingZeros++;
  }
  return trailingZeros;
}

/**
 * Epoch schedule
 * (see https://docs.solana.com/terminology#epoch)
 */
export class EpochSchedule {
  /** The maximum number of slots in each epoch */
  public slotsPerEpoch: number;
  /** The number of slots before beginning of an epoch to calculate a leader schedule for that epoch */
  public leaderScheduleSlotOffset: number;
  /** Indicates whether epochs start short and grow */
  public warmup: boolean;
  /** The first epoch with `slotsPerEpoch` slots */
  public firstNormalEpoch: number;
  /** The first slot of `firstNormalEpoch` */
  public firstNormalSlot: number;

  constructor(
    slotsPerEpoch: number,
    leaderScheduleSlotOffset: number,
    warmup: boolean,
    firstNormalEpoch: number,
    firstNormalSlot: number,
  ) {
    this.slotsPerEpoch = slotsPerEpoch;
    this.leaderScheduleSlotOffset = leaderScheduleSlotOffset;
    this.warmup = warmup;
    this.firstNormalEpoch = firstNormalEpoch;
    this.firstNormalSlot = firstNormalSlot;
  }

  getFirstSlotInEpoch(epoch: number): number {
    if (epoch <= this.firstNormalEpoch) {
      return (Math.pow(2, epoch) - 1) * MINIMUM_SLOT_PER_EPOCH;
    } else {
      return (
        (epoch - this.firstNormalEpoch) * this.slotsPerEpoch +
        this.firstNormalSlot
      );
    }
  }

  getLastSlotInEpoch(epoch: number): number {
    return this.getFirstSlotInEpoch(epoch) + this.getSlotsInEpoch(epoch) - 1;
  }

  getSlotsInEpoch(epoch: number) {
    if (epoch < this.firstNormalEpoch) {
      return Math.pow(2, epoch + trailingZeros(MINIMUM_SLOT_PER_EPOCH));
    } else {
      return this.slotsPerEpoch;
    }
  }
}
