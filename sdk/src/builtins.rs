//! Solana helper macros for declaring built-in programs.

#[rustversion::since(1.46.0)]
#[macro_export]
macro_rules! declare_builtin_name {
    ($name:ident, $id:path, $entrypoint:expr) => {
        #[macro_export]
        macro_rules! $name {
            () => {
                // Subtle:
                // The outer `declare_builtin_name!` macro may be expanded in another
                // crate, causing the macro `$name!` to be defined in that
                // crate. We want to emit a call to `$crate::id()`, and have
                // `$crate` be resolved in the crate where `$name!` gets defined,
                // *not* in this crate (where `declare_builtin_name! is defined).
                //
                // When a macro_rules! macro gets expanded, any $crate tokens
                // in its output will be 'marked' with the crate they were expanded
                // from. This includes nested macros like our macro `$name` - even
                // though it looks like a separate macro, Rust considers it to be
                // just another part of the output of `declare_program!`.
                //
                // We pass `$name` as the second argument to tell `respan!` to
                // apply use the `Span` of `$name` when resolving `$crate::id`.
                // This causes `$crate` to behave as though it was written
                // at the same location as the `$name` value passed
                // to `declare_builtin_name!` (e.g. the 'foo' in
                // `declare_builtin_name(foo)`
                //
                // See the `respan!` macro for more details.
                // This should use `crate::respan!` once
                // https://github.com/rust-lang/rust/pull/72121 is merged:
                // see https://github.com/solana-labs/solana/issues/10933.
                // For now, we need to use `::solana_sdk`
                //
                // `respan!` respans the path `$crate::id`, which we then call (hence the extra
                // parens)
                (
                    stringify!($name).to_string(),
                    ::solana_sdk::respan!($crate::$id, $name)(),
                    $entrypoint,
                )
            };
        }
    };
}

#[rustversion::not(since(1.46.0))]
#[macro_export]
macro_rules! declare_builtin_name {
    ($name:ident, $id:path, $entrypoint:expr) => {
        #[macro_export]
        macro_rules! $name {
            () => {
                (stringify!($name).to_string(), $crate::$id(), $entrypoint)
            };
        }
    };
}

/// Convenience macro to declare a built-in program.
///
/// bs58_string: bs58 string representation the program's id
/// name: Name of the program
/// entrypoint: Program's entrypoint, must be of `type Entrypoint`
/// id: Path to the program id access function, used if this macro is not
///     called in `src/lib`
#[macro_export]
macro_rules! declare_builtin {
    ($bs58_string:expr, $name:ident, $entrypoint:expr) => {
        $crate::declare_builtin!($bs58_string, $name, $entrypoint, id);
    };
    ($bs58_string:expr, $name:ident, $entrypoint:expr, $id:path) => {
        $crate::declare_id!($bs58_string);
        $crate::declare_builtin_name!($name, $id, $entrypoint);
    };
}
