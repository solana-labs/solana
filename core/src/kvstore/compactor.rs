use crate::kvstore::error::{Error, Result};
use crate::kvstore::mapper::{Kind, Mapper};
use crate::kvstore::sstable::{Key, Merged, SSTable};

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

type TableVec = Vec<BTreeMap<Key, SSTable>>;
type TableSlice<'a> = &'a [BTreeMap<Key, SSTable>];

#[derive(Debug, Copy, Clone)]
pub struct Config {
    pub max_pages: usize,
    pub page_size: usize,
}

#[derive(Debug)]
pub enum Req {
    Start(PathBuf),
    Gc,
}

#[derive(Debug)]
pub enum Resp {
    Done(TableVec),
    Failed(Error),
}

pub fn spawn_compactor(
    mapper: Arc<dyn Mapper>,
    config: Config,
) -> Result<(Sender<Req>, Receiver<Resp>, JoinHandle<()>)> {
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();

    let handle = thread::spawn(move || {
        let _ignored = run_loop(mapper, config, req_rx, resp_tx);
    });

    Ok((req_tx, resp_rx, handle))
}

fn run_loop(
    mapper: Arc<dyn Mapper>,
    config: Config,
    req_rx: Receiver<Req>,
    resp_tx: Sender<Resp>,
) -> Result<()> {
    while let Ok(msg) = req_rx.recv() {
        match msg {
            Req::Start(_) => {
                let new_tables_res = run_compaction(&*mapper, &config);

                match new_tables_res {
                    Ok(new_tables) => {
                        resp_tx.send(Resp::Done(new_tables))?;
                    }
                    Err(e) => {
                        resp_tx.send(Resp::Failed(e))?;
                    }
                }
            }
            Req::Gc => {
                let _ = mapper.empty_trash();
            }
        }
    }

    Ok(())
}

fn run_compaction(mapper: &dyn Mapper, config: &Config) -> Result<TableVec> {
    let mut tables = load_tables(mapper)?;

    compact_level_0(mapper, &mut tables, config)?;

    for level in 1..tables.len() {
        while level_needs_compact(level as u8, config, &tables) {
            compact_upper_level(mapper, &mut tables, config, level as u8)?;
        }
    }

    // move old tables to garbage
    mapper.rotate_tables()?;

    Ok(tables)
}

fn compact_level_0(mapper: &dyn Mapper, tables: &mut TableVec, config: &Config) -> Result<()> {
    assert!(!tables.is_empty());

    if tables.len() == 1 {
        tables.push(BTreeMap::new());
    }

    let mut new_tables = BTreeMap::new();
    {
        let sources = tables
            .iter()
            .take(2)
            .map(BTreeMap::values)
            .flatten()
            .map(|sst| sst.range(&(Key::ALL_INCLUSIVE)))
            .collect::<Result<Vec<_>>>()?;

        let mut iter = Merged::new(sources).peekable();
        while iter.peek().is_some() {
            let sst = mapper.make_table(Kind::Compaction, &mut |mut data_wtr, mut index_wtr| {
                SSTable::create_capped(
                    &mut iter,
                    1,
                    config.page_size as u64,
                    &mut data_wtr,
                    &mut index_wtr,
                );
            })?;

            new_tables.insert(sst.meta().start, sst);
        }
    }

    tables[0].clear();
    tables[1].clear();

    tables[1].append(&mut new_tables);

    Ok(())
}

fn compact_upper_level(
    mapper: &dyn Mapper,
    pages: &mut TableVec,
    config: &Config,
    level: u8,
) -> Result<()> {
    assert!(1 <= level && (level as usize) < pages.len());
    assert!(!pages[level as usize].is_empty());

    let next_level = level + 1;
    let level = level as usize;

    if next_level as usize == pages.len() {
        pages.push(BTreeMap::new());
    }

    let (&key, chosen_sst) = pages[level].iter().next_back().unwrap();
    let (start, end) = {
        let meta = chosen_sst.meta();
        (meta.start, meta.end)
    };

    let mut page_keys = Vec::new();
    let mut merge_with = Vec::new();

    for (key, sst) in pages[next_level as usize].iter() {
        if sst.is_overlap(&(start..=end)) {
            page_keys.push(*key);
            merge_with.push(sst);
        }
    }

    let mut new_tables = BTreeMap::new();
    {
        let sources = merge_with
            .into_iter()
            .chain(std::iter::once(chosen_sst))
            .map(|sst| sst.range(&(Key::ALL_INCLUSIVE)))
            .collect::<Result<Vec<_>>>()?;

        let mut iter = Merged::new(sources).peekable();

        while iter.peek().is_some() {
            let sst = mapper.make_table(Kind::Compaction, &mut |mut data_wtr, mut index_wtr| {
                SSTable::create_capped(
                    &mut iter,
                    next_level,
                    config.page_size as u64,
                    &mut data_wtr,
                    &mut index_wtr,
                );
            })?;

            new_tables.insert(sst.meta().start, sst);
        }
    }

    // delete merged page and merged pages in next level
    pages[level].remove(&key).unwrap();

    for start_key in page_keys {
        pages[next_level as usize].remove(&start_key).unwrap();
    }

    pages[next_level as usize].append(&mut new_tables);

    Ok(())
}

fn load_tables(mapper: &dyn Mapper) -> Result<TableVec> {
    Ok(SSTable::sorted_tables(&mapper.active_set()?))
}

#[inline]
fn level_max(level: u8, config: &Config) -> usize {
    match level {
        0 => config.max_pages,
        x => 10usize.pow(u32::from(x)),
    }
}

#[inline]
fn level_needs_compact(level: u8, config: &Config, tables: TableSlice) -> bool {
    if level as usize >= tables.len() {
        return false;
    }

    let max = level_max(level, config);

    tables[level as usize].len() > max
}
