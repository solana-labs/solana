//! Lamports credited to this address will be removed from the total supply (burned) at the end of
//! the current block.
//!
//! Note that the incinerator is subject to rent collection, just like all other accounts.  This
//! means that if less than a rent-exempt amount of lamports is credited to the incinerator, rent
//! will be collected first and thus that rent amount will not be burned by the incinerator.
//!

crate::declare_id!("1nc1nerator11111111111111111111111111111111");
