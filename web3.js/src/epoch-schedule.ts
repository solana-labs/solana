const MINIMUM_SLOT_PER_EPOCH = 32;

// Returns the number of trailing zeros in the binary representation of self.
function trailingZeros(n: number) {
  let trailingZeros = 0;
  while (n > 1) {
    n /= 2;
    trailingZeros++;
  }
  return trailingZeros;
}

// Returns the smallest power of two greater than or equal to n
function nextPowerOfTwo(n: number) {
  if (n === 0) return 1;
  n--;
  n |= n >> 1;
  n |= n >> 2;
  n |= n >> 4;
  n |= n >> 8;
  n |= n >> 16;
  n |= n >> 32;
  return n + 1;
}

/**
 * Epoch schedule
 * (see https://docs.solana.com/terminology#epoch)
 * Can be retrieved with the {@link Connection.getEpochSchedule} method
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

  getEpoch(slot: number): number {
    return this.getEpochAndSlotIndex(slot)[0];
  }

  getEpochAndSlotIndex(slot: number): [number, number] {
    if (slot < this.firstNormalSlot) {
      const epoch =
        trailingZeros(nextPowerOfTwo(slot + MINIMUM_SLOT_PER_EPOCH + 1)) -
        trailingZeros(MINIMUM_SLOT_PER_EPOCH) -
        1;

      const epochLen = this.getSlotsInEpoch(epoch);
      const slotIndex = slot - (epochLen - MINIMUM_SLOT_PER_EPOCH);
      return [epoch, slotIndex];
    } else {
      const normalSlotIndex = slot - this.firstNormalSlot;
      const normalEpochIndex = Math.floor(normalSlotIndex / this.slotsPerEpoch);
      const epoch = this.firstNormalEpoch + normalEpochIndex;
      const slotIndex = normalSlotIndex % this.slotsPerEpoch;
      return [epoch, slotIndex];
    }
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
