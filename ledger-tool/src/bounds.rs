use {
    clap::{App, Arg, ArgMatches, SubCommand},
    serde::Serialize,
    solana_cli_output::{OutputFormat, QuietDisplay, VerboseDisplay},
    solana_ledger::blockstore::Blockstore,
    std::{
        fmt::{Display, Formatter, Result},
        process::exit,
    },
};

pub trait BoundsSubCommand {
    fn bounds_subcommand(self) -> Self;
}

impl BoundsSubCommand for App<'_, '_> {
    fn bounds_subcommand(self) -> Self {
        self.subcommand(
            SubCommand::with_name("bounds")
                .about(
                    "Print lowest and highest non-empty slots. \
                    Note that there may be empty slots within the bounds",
                )
                .arg(
                    Arg::with_name("all")
                        .long("all")
                        .takes_value(false)
                        .required(false)
                        .help("Additionally print all the non-empty slots within the bounds"),
                ),
        )
    }
}

#[derive(Serialize, Debug, Default)]
pub struct SlotInfo {
    start: u64,
    end: u64,
    total: usize,
    #[serde(rename = "fromLast", skip_serializing_if = "Option::is_none")]
    from_last: Option<u64>,
}

impl Display for SlotInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(f, "Start: {}", &self.start)?;
        writeln!(f, "End: {}", &self.end)?;
        writeln!(f, "Total: {}", &self.total)?;
        if let Some(from_last) = &self.from_last {
            writeln!(f, "From Last: {}", from_last)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlotBounds {
    #[serde(skip_serializing_if = "Option::is_none")]
    total_slots: Option<SlotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rooted_slots: Option<SlotInfo>,
}

impl VerboseDisplay for SlotBounds {}
impl QuietDisplay for SlotBounds {}

impl Display for SlotBounds {
    fn fmt(&self, f: &mut Formatter) -> Result {
        if let Some(total_slots) = &self.total_slots {
            writeln!(f, "Total Slots: {}", total_slots)?;

            if let Some(rooted_slots) = &self.rooted_slots {
                writeln!(f, "Rooted Slots: {}", rooted_slots)?;
            }
        }
        Ok(())
    }
}

pub fn bounds_process_command(blockstore: Blockstore, matches: &ArgMatches) {
    match blockstore.slot_meta_iterator(0) {
        Ok(metas) => {
            let output_format = OutputFormat::from_matches(matches, "output_format", false);
            let all = matches.is_present("all");

            let slots: Vec<_> = metas.map(|(slot, _)| slot).collect();
            let mut slot_bounds = SlotBounds::default();

            if slots.is_empty() {
                match output_format {
                    OutputFormat::Json | OutputFormat::JsonCompact => {
                        println!("{}", output_format.formatted_string(&slot_bounds))
                    }
                    _ => println!("Ledger is empty"),
                }
            } else {
                let first = slots.first().unwrap();
                let last = slots.last().unwrap_or(first);

                slot_bounds.total_slots = Some(SlotInfo {
                    start: *first,
                    end: *last,
                    total: slots.len(),
                    ..Default::default()
                });

                // Rooted Slots
                if let Ok(rooted) = blockstore.rooted_slot_iterator(0) {
                    let mut first_rooted = 0;
                    let mut last_rooted = 0;
                    let mut total_rooted = 0;
                    for (i, slot) in rooted.into_iter().enumerate() {
                        if i == 0 {
                            first_rooted = slot;
                        }
                        last_rooted = slot;
                        total_rooted += 1;
                    }
                    let mut count_past_root = 0;
                    for slot in slots.iter().rev() {
                        if *slot > last_rooted {
                            count_past_root += 1;
                        } else {
                            break;
                        }
                    }

                    slot_bounds.rooted_slots = Some(SlotInfo {
                        start: first_rooted,
                        end: last_rooted,
                        total: total_rooted,
                        from_last: Some(count_past_root),
                    });
                }
            }

            // Print collected data
            match output_format {
                OutputFormat::Json | OutputFormat::JsonCompact => {
                    println!("{}", output_format.formatted_string(&slot_bounds))
                }
                _ => {
                    // Simple text print in other cases.

                    if let Some(total_slots) = slot_bounds.total_slots {
                        if total_slots.start != total_slots.end {
                            println!(
                                "Ledger has data for {:?} slots {:?} to {:?}",
                                total_slots.total, total_slots.start, total_slots.end
                            );
                            if all {
                                println!("Non-empty slots: {:?}", slots);
                            }
                        } else {
                            println!("Ledger has data for slot {:?}", total_slots.start);
                        }

                        if let Some(rooted_slots) = slot_bounds.rooted_slots {
                            println!(
                                "  with {:?} rooted slots from {:?} to {:?}",
                                rooted_slots.total, rooted_slots.start, rooted_slots.end
                            );
                            println!(
                                "  and {:?} slots past the last root",
                                rooted_slots.from_last
                            )
                        } else {
                            println!("  with no rooted slots")
                        }
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("Unable to read the Ledger: {:?}", err);
            exit(1);
        }
    };
}
