use {
    crate::gcov::{GcovFile, GcovIntermediate, GcovLine},
    bv::{BitVec, BitsMut},
    gimli::{Dwarf, EndianSlice, LineProgramHeader, RunTimeEndian, Unit, UnitHeader},
    goblin::elf::Elf,
    itertools::Itertools,
    log::*,
    std::{
        borrow::Cow,
        collections::{BTreeSet, HashMap},
        fmt::{Debug, Formatter},
        path::{Path, PathBuf},
    },
};

#[derive(Default)]
pub(crate) struct FileCoverage {
    file_path: Option<PathBuf>,
    hits: BTreeSet<(u64, u64)>,
}

impl FileCoverage {
    pub(crate) fn new(file_path: Option<PathBuf>) -> Self {
        Self {
            file_path,
            ..FileCoverage::default()
        }
    }
}

#[derive(Default)]
pub(crate) struct Coverage {
    hits: HashMap<u64, FileCoverage>,
}

impl Coverage {
    pub(crate) fn from_trace(
        elf_bytes: &[u8],
        elf: &Elf<'_>,
        trace: &[[u64; 12]],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Find text section.
        let text_range = elf
            .section_headers
            .iter()
            .find(|section| elf.shdr_strtab.get_at(section.sh_name) == Some(".text"))
            .ok_or("missing .text section")?
            .file_range()
            .ok_or("invalid .text range")?;

        // Create bitmap of executed instructions.
        let mut hits = BitVec::<usize>::new_fill(false, (text_range.len() / 8) as u64);
        for ins in trace {
            hits.set_bit(ins[11], true);
        }

        // Teach gimli how to load a section from goblin.
        let load_section = |id: gimli::SectionId| -> Result<Cow<[u8]>, gimli::Error> {
            let file_range = elf
                .section_headers
                .iter()
                .find(|section| {
                    let section_name = elf.shdr_strtab.get_at(section.sh_name);
                    section_name == Some(id.name())
                })
                .and_then(|section| section.file_range());
            Ok(match file_range {
                None => {
                    debug!("Section {} not found", id.name());
                    Cow::Borrowed(&[][..])
                }
                Some(file_range) => {
                    let section_bytes = &elf_bytes[file_range.start..file_range.end];
                    debug!("Section {}: {} bytes", id.name(), section_bytes.len());
                    Cow::Borrowed(section_bytes)
                }
            })
        };

        // Teach gimli how to switch endianness when needed.
        let borrow_section: &dyn for<'a> Fn(
            &'a Cow<[u8]>,
        )
            -> gimli::EndianSlice<'a, gimli::RunTimeEndian> =
            &|section| gimli::EndianSlice::new(&*section, gimli::RunTimeEndian::Little);

        // Load all of the sections.
        let dwarf_cow = Dwarf::load(&load_section)?;

        // Create `EndianSlice`s for all of the sections.
        let dwarf = dwarf_cow.borrow(&borrow_section);

        let mut cov = Self::default();

        // Iterate over the compilation units.
        let mut iter = dwarf.units();
        while let Some(header) = iter.next()? {
            if let Err(e) = cov.process_unit(&dwarf, header, text_range.start as u64, &hits) {
                error!("Failed to extract coverage from compile unit: {:?}", e);
            }
        }

        Ok(cov)
    }

    fn process_unit(
        &mut self,
        dwarf: &Dwarf<EndianSlice<'_, RunTimeEndian>>,
        header: UnitHeader<EndianSlice<'_, RunTimeEndian>, usize>,
        text_section_offset: u64,
        hits: &bv::BitVec,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(
            "Line number info for unit at <.debug_info+0x{:x}>",
            header.offset().as_debug_info_offset().unwrap().0
        );
        let unit = dwarf.unit(header)?;

        // Get the line program for the compilation unit.
        let program = match unit.line_program.clone() {
            None => return Ok(()),
            Some(program) => program,
        };

        let comp_dir = if let Some(ref dir) = unit.comp_dir {
            PathBuf::from(dir.to_string_lossy().into_owned())
        } else {
            PathBuf::new()
        };

        // Iterate over the line program rows.
        let mut rows = program.rows();
        while let Some((header, row)) = rows.next_row()? {
            if row.end_sequence() {
                warn!(
                    "Possible gap in addresses: {:x} end-sequence",
                    row.address()
                );
                continue;
            }

            // Determine line/column. DWARF line/column is never 0, so we use that
            // but other applications may want to display this differently.
            let line = match row.line() {
                Some(line) => line.get(),
                None => 0,
            };
            let column = match row.column() {
                gimli::ColumnType::LeftEdge => 0,
                gimli::ColumnType::Column(column) => column.get(),
            };

            if let Some(ins_index) = row
                .address()
                .checked_sub(text_section_offset)
                .map(|x| x / 8)
            {
                if hits[ins_index] {
                    self.file_coverage(&comp_dir, dwarf, &unit, row, header)
                        .hits
                        .insert((line, column));
                }
            }
        }

        Ok(())
    }

    fn file_coverage(
        &mut self,
        comp_dir: &Path,
        dwarf: &Dwarf<EndianSlice<'_, RunTimeEndian>>,
        unit: &Unit<EndianSlice<'_, RunTimeEndian>>,
        row: &gimli::LineRow,
        header: &LineProgramHeader<EndianSlice<'_, RunTimeEndian>, usize>,
    ) -> &mut FileCoverage {
        let file_index = row.file_index();
        self.hits.entry(file_index).or_insert_with(|| {
            // Create new FileCoverage object.
            // Read path from ELF.
            let file_path = row.file(header).and_then(|file| {
                let mut path = PathBuf::from(comp_dir);

                // The directory index 0 is defined to correspond to the compilation unit directory.
                if file.directory_index() != 0 {
                    if let Some(dir) = file.directory(header) {
                        path.push(
                            dwarf
                                .attr_string(unit, dir)
                                .ok()?
                                .to_string_lossy()
                                .as_ref(),
                        );
                    }
                }

                path.push(
                    dwarf
                        .attr_string(unit, file.path_name())
                        .ok()?
                        .to_string_lossy()
                        .as_ref(),
                );

                Some(path)
            });
            // Return newly created file cov object.
            FileCoverage::new(file_path)
        })
    }
}

impl Debug for Coverage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for file_cov in self.hits.values() {
            let file_path = match file_cov.file_path.as_ref() {
                Some(p) => p,
                None => continue,
            };
            for (line, number) in &file_cov.hits {
                writeln!(f, "file={:?} line={} col={}", file_path, line, number)?;
            }
        }
        Ok(())
    }
}

impl From<&Coverage> for GcovIntermediate {
    fn from(cov: &Coverage) -> Self {
        GcovIntermediate {
            files: cov.hits.values().map(|file| file.into()).collect(),
        }
    }
}

impl From<&FileCoverage> for GcovFile {
    fn from(cov: &FileCoverage) -> Self {
        let lines = cov
            .hits
            .iter()
            .group_by(|(line, _)| line)
            .into_iter()
            .map(|(line, cols)| GcovLine {
                line_number: *line,
                count: cols.count(), // TODO count actual hits here, not cols
            })
            .collect::<Vec<_>>();
        GcovFile {
            file: cov
                .file_path
                .as_ref()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| "".to_string()),
            lines,
        }
    }
}
