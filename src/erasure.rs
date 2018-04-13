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
        let mut elems = Vec::new();
        let mut valid = Vec::new();
        for b in window.iter_mut() {
            if b.is_some() {
                valid.push(1u8);
                continue;
            }
            let n = re.allocate();
            *b = Some(n);
            //mark the missing memory
            valid.push(0u8);
        }
        //lock everything
        for b in window.clone().into_iter() {
            for x in b.into_iter() {
                locks.push(x.read().unwrap());
            }
        }
        for l in locks.iter() {
            elems.push(l.data.as_ptr());
        }
        //TODO: call out to erasure with elems
    }
    Ok(())
}
