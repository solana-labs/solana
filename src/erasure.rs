//TODO(sakridge) pick these values
const NUM_CODED: usize = 10
const MAX_MISSING: usize = 1

fn recover(re: BlobRecycler, window: &mut Vec<Option<SharedBlob>>, consumed: usize) -> Result<()> {
    //recover with erasure coding
    let mut available = 0;
    let mut missing = 0;
    for i in consumed .. (consumed + NUM_CODED) {
        let n = i % window.len();
        if window[n].is_some() {
            available += 1;
        } else {
            missing += 1;
        }
    }
    if missing > 0 && missing < MAX_MISSING {
        let mut locks = Vec::new();
        let mut elems = Vec::new();
        let mut valid = Vec::new();
        for b in window.iter_mut() {
            if b.is_some() {
                valid.push(1);
                continue;
            }
            let n = re.allocate();
            *b = Some(n);
            //mark the missing memory
            valid.push(0);
        }
        //lock everything
        for b in window.iter_mut() {
            let l = b.unwrap().read().unwrap();
            locks.push(l);
            elems.push(Some(l.dat).as_ptr()));
        }
        //TODO: call out to erasure with elems
    }
}
