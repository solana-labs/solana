//TODO(sakridge) pick these values
use packet::{SharedBlob, BlobRecycler};
use result::Result;

const NUM_CODED: usize = 10;
const MAX_MISSING: usize = 1;

pub fn recover(re: BlobRecycler, window: &mut Vec<Option<SharedBlob>>, consumed: usize) -> Result<()> {
    //recover with erasure coding
    let mut missing = 0;
    for i in consumed .. (consumed + NUM_CODED) {
        let n = i % window.len();
        if window[n].is_none() {
            missing += 1;
        }
    }
    if missing > 0 && missing < MAX_MISSING {
        let mut locks = Vec::new();
        let mut ptrs = Vec::new();
        let mut blobs: Vec<SharedBlob> = Vec::new();
        let mut valid = Vec::new();
        for b in window.iter_mut() {
            if b.is_some() {
                valid.push(1u8);
                blobs.push(b.unwrap().clone());
                continue;
            }
            let n = re.allocate();
            *b = Some(n.clone());
            //mark the missing memory
            valid.push(0u8);
            blobs.push(n);
        }
        //lock everything
        for b in blobs.into_iter() {
            locks.push(b.read().unwrap());
        }
        for l in locks.iter() {
            ptrs.push(l.data.as_ptr());
        }
        //TODO: call out to erasure with ptrs
    }
    Ok(())
}
