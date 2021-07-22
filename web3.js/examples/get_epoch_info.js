import * as solanaWeb3 from '@solana/web3.js';

export async function logEpochInfo () {
    let conn = new solanaWeb3.Connection(clusterApiUrl('mainnet-beta')); //mainnet-beta or devnet -- connects to the network
    let epochinfo = await conn.getEpochInfo(); 
    /*returns an object
      absoluteSlot: <u64>, the current slot
      blockHeight: <u64>, the current block height
      epoch: <u64>, the current epoch
      slotIndex: <u64>, the current slot relative to the start of the current epoch
      slotsInEpoch: <u64>, the number of slots in this epoch
    */
  
    console.log('absoluteSlot: '  + epochinfo.absoluteSlot);  //the current slot
    console.log('blockHeight: '   + epochinfo.blockHeight);   //the current block height
    console.log('epoch: '         + epochinfo.epoch);         //the current epoch
    console.log('slotIndex: '     + epochinfo.slotIndex);     //the current slot relative to the start of the current epoch
    console.log('slotsInEpoch: '  + epochinfo.slotsInEpoch);  //the number of slots in this epoch
}
