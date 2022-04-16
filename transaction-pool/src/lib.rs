use solana::sdk::Pubkey;

struct Item {
    priority: u64,
    units: u64,
    time: u64,
    id: (u32,u32),
}

struct Pool {
    max_age: u64,
    max_bucket_cu, u64,
    max_block_cu, u64,
    items: BTreeMap<u64, VecDeque<Item>>,
}

trait Table {
    fn keys(&self, item: &Item) -> &[&Pubkey],
}

impl Pool {
    pub fn insert(&mut self, item: Item) {
        let bucket = self.items.entry(item.priority).or_insert_with(VecDqueue::new);
        bucket.push_back(item);
    }
    pub fn pop_block<T: Table>(&mut self, time: u64, table: &Table) -> VecDeque<Item> {
        let mut rv = VecDeque::new();
        let mut total_cu = 0;
        let mut buckets: HashMap<Pubkey, u64> = HashMap::new();
        let mut gc = vec![];
        for (k,v) in &mut self.items.iter_mut() {
            while v.font().is_some() && total_cu < self.max_block_cu {
                let item = v.front().unwrap();
                if time - item.time > self.max_age {
                    v.pop_front();
                    break;
                }
                let mut fail = false;
                let keys = table.keys(item); 
                for k in keys {
                    if buckets[k] + item.units > self.max_bucket_cu {
                        fail = true;
                        break;
                    }
                }
                if fail {
                    continue;
                }
                for k in keys {
                    buckets[k] += item.units;
                }
                total_cu += item.units;
                rv.push_back(v.pop_front());
            }
            if v.front().is_none() {
                gc.push(k);
            }
            if total_cu >= self.max_block_cu {
                break;
            }
        }
    }
}
