/// Specifies the complete (requested) contents of a single row of a table.
/// Rows which exceed 256MiB in size cannot be read in full.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Row {
    /// The unique key which identifies this row within its table. This is the same
    /// key that's used to identify the row in, for example, a MutateRowRequest.
    /// May contain any non-empty byte string up to 4KiB in length.
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// May be empty, but only if the entire row is empty.
    /// The mutual ordering of column families is not specified.
    #[prost(message, repeated, tag = "2")]
    pub families: ::prost::alloc::vec::Vec<Family>,
}
/// Specifies (some of) the contents of a single row/column family intersection
/// of a table.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Family {
    /// The unique key which identifies this family within its row. This is the
    /// same key that's used to identify the family in, for example, a RowFilter
    /// which sets its "family_name_regex_filter" field.
    /// Must match `\[-_.a-zA-Z0-9\]+`, except that AggregatingRowProcessors may
    /// produce cells in a sentinel family with an empty name.
    /// Must be no greater than 64 characters in length.
    #[prost(string, tag = "1")]
    pub name: ::prost::alloc::string::String,
    /// Must not be empty. Sorted in order of increasing "qualifier".
    #[prost(message, repeated, tag = "2")]
    pub columns: ::prost::alloc::vec::Vec<Column>,
}
/// Specifies (some of) the contents of a single row/column intersection of a
/// table.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Column {
    /// The unique key which identifies this column within its family. This is the
    /// same key that's used to identify the column in, for example, a RowFilter
    /// which sets its `column_qualifier_regex_filter` field.
    /// May contain any byte string, including the empty string, up to 16kiB in
    /// length.
    #[prost(bytes = "vec", tag = "1")]
    pub qualifier: ::prost::alloc::vec::Vec<u8>,
    /// Must not be empty. Sorted in order of decreasing "timestamp_micros".
    #[prost(message, repeated, tag = "2")]
    pub cells: ::prost::alloc::vec::Vec<Cell>,
}
/// Specifies (some of) the contents of a single row/column/timestamp of a table.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Cell {
    /// The cell's stored timestamp, which also uniquely identifies it within
    /// its column.
    /// Values are always expressed in microseconds, but individual tables may set
    /// a coarser granularity to further restrict the allowed values. For
    /// example, a table which specifies millisecond granularity will only allow
    /// values of `timestamp_micros` which are multiples of 1000.
    #[prost(int64, tag = "1")]
    pub timestamp_micros: i64,
    /// The value stored in the cell.
    /// May contain any byte string, including the empty string, up to 100MiB in
    /// length.
    #[prost(bytes = "vec", tag = "2")]
    pub value: ::prost::alloc::vec::Vec<u8>,
    /// Labels applied to the cell by a \[RowFilter][google.bigtable.v2.RowFilter\].
    #[prost(string, repeated, tag = "3")]
    pub labels: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// Specifies a contiguous range of rows.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RowRange {
    /// The row key at which to start the range.
    /// If neither field is set, interpreted as the empty string, inclusive.
    #[prost(oneof = "row_range::StartKey", tags = "1, 2")]
    pub start_key: ::core::option::Option<row_range::StartKey>,
    /// The row key at which to end the range.
    /// If neither field is set, interpreted as the infinite row key, exclusive.
    #[prost(oneof = "row_range::EndKey", tags = "3, 4")]
    pub end_key: ::core::option::Option<row_range::EndKey>,
}
/// Nested message and enum types in `RowRange`.
pub mod row_range {
    /// The row key at which to start the range.
    /// If neither field is set, interpreted as the empty string, inclusive.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StartKey {
        /// Used when giving an inclusive lower bound for the range.
        #[prost(bytes, tag = "1")]
        StartKeyClosed(::prost::alloc::vec::Vec<u8>),
        /// Used when giving an exclusive lower bound for the range.
        #[prost(bytes, tag = "2")]
        StartKeyOpen(::prost::alloc::vec::Vec<u8>),
    }
    /// The row key at which to end the range.
    /// If neither field is set, interpreted as the infinite row key, exclusive.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum EndKey {
        /// Used when giving an exclusive upper bound for the range.
        #[prost(bytes, tag = "3")]
        EndKeyOpen(::prost::alloc::vec::Vec<u8>),
        /// Used when giving an inclusive upper bound for the range.
        #[prost(bytes, tag = "4")]
        EndKeyClosed(::prost::alloc::vec::Vec<u8>),
    }
}
/// Specifies a non-contiguous set of rows.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RowSet {
    /// Single rows included in the set.
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub row_keys: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
    /// Contiguous row ranges included in the set.
    #[prost(message, repeated, tag = "2")]
    pub row_ranges: ::prost::alloc::vec::Vec<RowRange>,
}
/// Specifies a contiguous range of columns within a single column family.
/// The range spans from &lt;column_family&gt;:&lt;start_qualifier&gt; to
/// &lt;column_family&gt;:&lt;end_qualifier&gt;, where both bounds can be either
/// inclusive or exclusive.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ColumnRange {
    /// The name of the column family within which this range falls.
    #[prost(string, tag = "1")]
    pub family_name: ::prost::alloc::string::String,
    /// The column qualifier at which to start the range (within `column_family`).
    /// If neither field is set, interpreted as the empty string, inclusive.
    #[prost(oneof = "column_range::StartQualifier", tags = "2, 3")]
    pub start_qualifier: ::core::option::Option<column_range::StartQualifier>,
    /// The column qualifier at which to end the range (within `column_family`).
    /// If neither field is set, interpreted as the infinite string, exclusive.
    #[prost(oneof = "column_range::EndQualifier", tags = "4, 5")]
    pub end_qualifier: ::core::option::Option<column_range::EndQualifier>,
}
/// Nested message and enum types in `ColumnRange`.
pub mod column_range {
    /// The column qualifier at which to start the range (within `column_family`).
    /// If neither field is set, interpreted as the empty string, inclusive.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StartQualifier {
        /// Used when giving an inclusive lower bound for the range.
        #[prost(bytes, tag = "2")]
        StartQualifierClosed(::prost::alloc::vec::Vec<u8>),
        /// Used when giving an exclusive lower bound for the range.
        #[prost(bytes, tag = "3")]
        StartQualifierOpen(::prost::alloc::vec::Vec<u8>),
    }
    /// The column qualifier at which to end the range (within `column_family`).
    /// If neither field is set, interpreted as the infinite string, exclusive.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum EndQualifier {
        /// Used when giving an inclusive upper bound for the range.
        #[prost(bytes, tag = "4")]
        EndQualifierClosed(::prost::alloc::vec::Vec<u8>),
        /// Used when giving an exclusive upper bound for the range.
        #[prost(bytes, tag = "5")]
        EndQualifierOpen(::prost::alloc::vec::Vec<u8>),
    }
}
/// Specified a contiguous range of microsecond timestamps.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TimestampRange {
    /// Inclusive lower bound. If left empty, interpreted as 0.
    #[prost(int64, tag = "1")]
    pub start_timestamp_micros: i64,
    /// Exclusive upper bound. If left empty, interpreted as infinity.
    #[prost(int64, tag = "2")]
    pub end_timestamp_micros: i64,
}
/// Specifies a contiguous range of raw byte values.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ValueRange {
    /// The value at which to start the range.
    /// If neither field is set, interpreted as the empty string, inclusive.
    #[prost(oneof = "value_range::StartValue", tags = "1, 2")]
    pub start_value: ::core::option::Option<value_range::StartValue>,
    /// The value at which to end the range.
    /// If neither field is set, interpreted as the infinite string, exclusive.
    #[prost(oneof = "value_range::EndValue", tags = "3, 4")]
    pub end_value: ::core::option::Option<value_range::EndValue>,
}
/// Nested message and enum types in `ValueRange`.
pub mod value_range {
    /// The value at which to start the range.
    /// If neither field is set, interpreted as the empty string, inclusive.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StartValue {
        /// Used when giving an inclusive lower bound for the range.
        #[prost(bytes, tag = "1")]
        StartValueClosed(::prost::alloc::vec::Vec<u8>),
        /// Used when giving an exclusive lower bound for the range.
        #[prost(bytes, tag = "2")]
        StartValueOpen(::prost::alloc::vec::Vec<u8>),
    }
    /// The value at which to end the range.
    /// If neither field is set, interpreted as the infinite string, exclusive.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum EndValue {
        /// Used when giving an inclusive upper bound for the range.
        #[prost(bytes, tag = "3")]
        EndValueClosed(::prost::alloc::vec::Vec<u8>),
        /// Used when giving an exclusive upper bound for the range.
        #[prost(bytes, tag = "4")]
        EndValueOpen(::prost::alloc::vec::Vec<u8>),
    }
}
/// Takes a row as input and produces an alternate view of the row based on
/// specified rules. For example, a RowFilter might trim down a row to include
/// just the cells from columns matching a given regular expression, or might
/// return all the cells of a row but not their values. More complicated filters
/// can be composed out of these components to express requests such as, "within
/// every column of a particular family, give just the two most recent cells
/// which are older than timestamp X."
///
/// There are two broad categories of RowFilters (true filters and transformers),
/// as well as two ways to compose simple filters into more complex ones
/// (chains and interleaves). They work as follows:
///
/// * True filters alter the input row by excluding some of its cells wholesale
/// from the output row. An example of a true filter is the `value_regex_filter`,
/// which excludes cells whose values don't match the specified pattern. All
/// regex true filters use RE2 syntax (<https://github.com/google/re2/wiki/Syntax>)
/// in raw byte mode (RE2::Latin1), and are evaluated as full matches. An
/// important point to keep in mind is that `RE2(.)` is equivalent by default to
/// `RE2(\[^\n\])`, meaning that it does not match newlines. When attempting to
/// match an arbitrary byte, you should therefore use the escape sequence `\C`,
/// which may need to be further escaped as `\\C` in your client language.
///
/// * Transformers alter the input row by changing the values of some of its
/// cells in the output, without excluding them completely. Currently, the only
/// supported transformer is the `strip_value_transformer`, which replaces every
/// cell's value with the empty string.
///
/// * Chains and interleaves are described in more detail in the
/// RowFilter.Chain and RowFilter.Interleave documentation.
///
/// The total serialized size of a RowFilter message must not
/// exceed 20480 bytes, and RowFilters may not be nested within each other
/// (in Chains or Interleaves) to a depth of more than 20.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RowFilter {
    /// Which of the possible RowFilter types to apply. If none are set, this
    /// RowFilter returns all cells in the input row.
    #[prost(
        oneof = "row_filter::Filter",
        tags = "1, 2, 3, 16, 17, 18, 4, 14, 5, 6, 7, 8, 9, 15, 10, 11, 12, 13, 19"
    )]
    pub filter: ::core::option::Option<row_filter::Filter>,
}
/// Nested message and enum types in `RowFilter`.
pub mod row_filter {
    /// A RowFilter which sends rows through several RowFilters in sequence.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Chain {
        /// The elements of "filters" are chained together to process the input row:
        /// in row -> f(0) -> intermediate row -> f(1) -> ... -> f(N) -> out row
        /// The full chain is executed atomically.
        #[prost(message, repeated, tag = "1")]
        pub filters: ::prost::alloc::vec::Vec<super::RowFilter>,
    }
    /// A RowFilter which sends each row to each of several component
    /// RowFilters and interleaves the results.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Interleave {
        /// The elements of "filters" all process a copy of the input row, and the
        /// results are pooled, sorted, and combined into a single output row.
        /// If multiple cells are produced with the same column and timestamp,
        /// they will all appear in the output row in an unspecified mutual order.
        /// Consider the following example, with three filters:
        ///```ignore
        ///                                   input row
        ///                                       |
        ///             -----------------------------------------------------
        ///             |                         |                         |
        ///            f(0)                      f(1)                      f(2)
        ///             |                         |                         |
        ///      1: foo,bar,10,x             foo,bar,10,z              far,bar,7,a
        ///      2: foo,blah,11,z            far,blah,5,x              far,blah,5,x
        ///             |                         |                         |
        ///             -----------------------------------------------------
        ///                                       |
        ///      1:                      foo,bar,10,z   // could have switched with #2
        ///      2:                      foo,bar,10,x   // could have switched with #1
        ///      3:                      foo,blah,11,z
        ///      4:                      far,bar,7,a
        ///      5:                      far,blah,5,x   // identical to #6
        ///      6:                      far,blah,5,x   // identical to #5
        ///
        /// All interleaved filters are executed atomically.
        #[prost(message, repeated, tag = "1")]
        pub filters: ::prost::alloc::vec::Vec<super::RowFilter>,
    }
    /// A RowFilter which evaluates one of two possible RowFilters, depending on
    /// whether or not a predicate RowFilter outputs any cells from the input row.
    ///
    /// IMPORTANT NOTE: The predicate filter does not execute atomically with the
    /// true and false filters, which may lead to inconsistent or unexpected
    /// results. Additionally, Condition filters have poor performance, especially
    /// when filters are set for the false condition.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Condition {
        /// If `predicate_filter` outputs any cells, then `true_filter` will be
        /// evaluated on the input row. Otherwise, `false_filter` will be evaluated.
        #[prost(message, optional, boxed, tag = "1")]
        pub predicate_filter: ::core::option::Option<
            ::prost::alloc::boxed::Box<super::RowFilter>,
        >,
        /// The filter to apply to the input row if `predicate_filter` returns any
        /// results. If not provided, no results will be returned in the true case.
        #[prost(message, optional, boxed, tag = "2")]
        pub true_filter: ::core::option::Option<
            ::prost::alloc::boxed::Box<super::RowFilter>,
        >,
        /// The filter to apply to the input row if `predicate_filter` does not
        /// return any results. If not provided, no results will be returned in the
        /// false case.
        #[prost(message, optional, boxed, tag = "3")]
        pub false_filter: ::core::option::Option<
            ::prost::alloc::boxed::Box<super::RowFilter>,
        >,
    }
    /// Which of the possible RowFilter types to apply. If none are set, this
    /// RowFilter returns all cells in the input row.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Filter {
        /// Applies several RowFilters to the data in sequence, progressively
        /// narrowing the results.
        #[prost(message, tag = "1")]
        Chain(Chain),
        /// Applies several RowFilters to the data in parallel and combines the
        /// results.
        #[prost(message, tag = "2")]
        Interleave(Interleave),
        /// Applies one of two possible RowFilters to the data based on the output of
        /// a predicate RowFilter.
        #[prost(message, tag = "3")]
        Condition(::prost::alloc::boxed::Box<Condition>),
        /// ADVANCED USE ONLY.
        /// Hook for introspection into the RowFilter. Outputs all cells directly to
        /// the output of the read rather than to any parent filter. Consider the
        /// following example:
        ///```ignore
        ///      Chain(
        ///        FamilyRegex("A"),
        ///        Interleave(
        ///          All(),
        ///          Chain(Label("foo"), Sink())
        ///        ),
        ///        QualifierRegex("B")
        ///      )
        ///
        ///                          A,A,1,w
        ///                          A,B,2,x
        ///                          B,B,4,z
        ///                             |
        ///                      FamilyRegex("A")
        ///                             |
        ///                          A,A,1,w
        ///                          A,B,2,x
        ///                             |
        ///                +------------+-------------+
        ///                |                          |
        ///              All()                    Label(foo)
        ///                |                          |
        ///             A,A,1,w              A,A,1,w,labels:\[foo\]
        ///             A,B,2,x              A,B,2,x,labels:\[foo\]
        ///                |                          |
        ///                |                        Sink() --------------+
        ///                |                          |                  |
        ///                +------------+      x------+          A,A,1,w,labels:\[foo\]
        ///                             |                        A,B,2,x,labels:\[foo\]
        ///                          A,A,1,w                             |
        ///                          A,B,2,x                             |
        ///                             |                                |
        ///                     QualifierRegex("B")                      |
        ///                             |                                |
        ///                          A,B,2,x                             |
        ///                             |                                |
        ///                             +--------------------------------+
        ///                             |
        ///                          A,A,1,w,labels:\[foo\]
        ///                          A,B,2,x,labels:\[foo\]  // could be switched
        ///                          A,B,2,x               // could be switched
        ///
        /// Despite being excluded by the qualifier filter, a copy of every cell
        /// that reaches the sink is present in the final result.
        ///
        /// As with an \[Interleave][google.bigtable.v2.RowFilter.Interleave\],
        /// duplicate cells are possible, and appear in an unspecified mutual order.
        /// In this case we have a duplicate with column "A:B" and timestamp 2,
        /// because one copy passed through the all filter while the other was
        /// passed through the label and sink. Note that one copy has label "foo",
        /// while the other does not.
        ///
        /// Cannot be used within the `predicate_filter`, `true_filter`, or
        /// `false_filter` of a \[Condition][google.bigtable.v2.RowFilter.Condition\].
        #[prost(bool, tag = "16")]
        Sink(bool),
        /// Matches all cells, regardless of input. Functionally equivalent to
        /// leaving `filter` unset, but included for completeness.
        #[prost(bool, tag = "17")]
        PassAllFilter(bool),
        /// Does not match any cells, regardless of input. Useful for temporarily
        /// disabling just part of a filter.
        #[prost(bool, tag = "18")]
        BlockAllFilter(bool),
        /// Matches only cells from rows whose keys satisfy the given RE2 regex. In
        /// other words, passes through the entire row when the key matches, and
        /// otherwise produces an empty row.
        /// Note that, since row keys can contain arbitrary bytes, the `\C` escape
        /// sequence must be used if a true wildcard is desired. The `.` character
        /// will not match the new line character `\n`, which may be present in a
        /// binary key.
        #[prost(bytes, tag = "4")]
        RowKeyRegexFilter(::prost::alloc::vec::Vec<u8>),
        /// Matches all cells from a row with probability p, and matches no cells
        /// from the row with probability 1-p.
        #[prost(double, tag = "14")]
        RowSampleFilter(f64),
        /// Matches only cells from columns whose families satisfy the given RE2
        /// regex. For technical reasons, the regex must not contain the `:`
        /// character, even if it is not being used as a literal.
        /// Note that, since column families cannot contain the new line character
        /// `\n`, it is sufficient to use `.` as a full wildcard when matching
        /// column family names.
        #[prost(string, tag = "5")]
        FamilyNameRegexFilter(::prost::alloc::string::String),
        /// Matches only cells from columns whose qualifiers satisfy the given RE2
        /// regex.
        /// Note that, since column qualifiers can contain arbitrary bytes, the `\C`
        /// escape sequence must be used if a true wildcard is desired. The `.`
        /// character will not match the new line character `\n`, which may be
        /// present in a binary qualifier.
        #[prost(bytes, tag = "6")]
        ColumnQualifierRegexFilter(::prost::alloc::vec::Vec<u8>),
        /// Matches only cells from columns within the given range.
        #[prost(message, tag = "7")]
        ColumnRangeFilter(super::ColumnRange),
        /// Matches only cells with timestamps within the given range.
        #[prost(message, tag = "8")]
        TimestampRangeFilter(super::TimestampRange),
        /// Matches only cells with values that satisfy the given regular expression.
        /// Note that, since cell values can contain arbitrary bytes, the `\C` escape
        /// sequence must be used if a true wildcard is desired. The `.` character
        /// will not match the new line character `\n`, which may be present in a
        /// binary value.
        #[prost(bytes, tag = "9")]
        ValueRegexFilter(::prost::alloc::vec::Vec<u8>),
        /// Matches only cells with values that fall within the given range.
        #[prost(message, tag = "15")]
        ValueRangeFilter(super::ValueRange),
        /// Skips the first N cells of each row, matching all subsequent cells.
        /// If duplicate cells are present, as is possible when using an Interleave,
        /// each copy of the cell is counted separately.
        #[prost(int32, tag = "10")]
        CellsPerRowOffsetFilter(i32),
        /// Matches only the first N cells of each row.
        /// If duplicate cells are present, as is possible when using an Interleave,
        /// each copy of the cell is counted separately.
        #[prost(int32, tag = "11")]
        CellsPerRowLimitFilter(i32),
        /// Matches only the most recent N cells within each column. For example,
        /// if N=2, this filter would match column `foo:bar` at timestamps 10 and 9,
        /// skip all earlier cells in `foo:bar`, and then begin matching again in
        /// column `foo:bar2`.
        /// If duplicate cells are present, as is possible when using an Interleave,
        /// each copy of the cell is counted separately.
        #[prost(int32, tag = "12")]
        CellsPerColumnLimitFilter(i32),
        /// Replaces each cell's value with the empty string.
        #[prost(bool, tag = "13")]
        StripValueTransformer(bool),
        /// Applies the given label to all cells in the output row. This allows
        /// the client to determine which results were produced from which part of
        /// the filter.
        ///
        /// Values must be at most 15 characters in length, and match the RE2
        /// pattern `\[a-z0-9\\-\]+`
        ///
        /// Due to a technical limitation, it is not currently possible to apply
        /// multiple labels to a cell. As a result, a Chain may have no more than
        /// one sub-filter which contains a `apply_label_transformer`. It is okay for
        /// an Interleave to contain multiple `apply_label_transformers`, as they
        /// will be applied to separate copies of the input. This may be relaxed in
        /// the future.
        #[prost(string, tag = "19")]
        ApplyLabelTransformer(::prost::alloc::string::String),
    }
}
/// Specifies a particular change to be made to the contents of a row.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Mutation {
    /// Which of the possible Mutation types to apply.
    #[prost(oneof = "mutation::Mutation", tags = "1, 2, 3, 4")]
    pub mutation: ::core::option::Option<mutation::Mutation>,
}
/// Nested message and enum types in `Mutation`.
pub mod mutation {
    /// A Mutation which sets the value of the specified cell.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct SetCell {
        /// The name of the family into which new data should be written.
        /// Must match `\[-_.a-zA-Z0-9\]+`
        #[prost(string, tag = "1")]
        pub family_name: ::prost::alloc::string::String,
        /// The qualifier of the column into which new data should be written.
        /// Can be any byte string, including the empty string.
        #[prost(bytes = "vec", tag = "2")]
        pub column_qualifier: ::prost::alloc::vec::Vec<u8>,
        /// The timestamp of the cell into which new data should be written.
        /// Use -1 for current Bigtable server time.
        /// Otherwise, the client should set this value itself, noting that the
        /// default value is a timestamp of zero if the field is left unspecified.
        /// Values must match the granularity of the table (e.g. micros, millis).
        #[prost(int64, tag = "3")]
        pub timestamp_micros: i64,
        /// The value to be written into the specified cell.
        #[prost(bytes = "vec", tag = "4")]
        pub value: ::prost::alloc::vec::Vec<u8>,
    }
    /// A Mutation which deletes cells from the specified column, optionally
    /// restricting the deletions to a given timestamp range.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeleteFromColumn {
        /// The name of the family from which cells should be deleted.
        /// Must match `\[-_.a-zA-Z0-9\]+`
        #[prost(string, tag = "1")]
        pub family_name: ::prost::alloc::string::String,
        /// The qualifier of the column from which cells should be deleted.
        /// Can be any byte string, including the empty string.
        #[prost(bytes = "vec", tag = "2")]
        pub column_qualifier: ::prost::alloc::vec::Vec<u8>,
        /// The range of timestamps within which cells should be deleted.
        #[prost(message, optional, tag = "3")]
        pub time_range: ::core::option::Option<super::TimestampRange>,
    }
    /// A Mutation which deletes all cells from the specified column family.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeleteFromFamily {
        /// The name of the family from which cells should be deleted.
        /// Must match `\[-_.a-zA-Z0-9\]+`
        #[prost(string, tag = "1")]
        pub family_name: ::prost::alloc::string::String,
    }
    /// A Mutation which deletes all cells from the containing row.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeleteFromRow {}
    /// Which of the possible Mutation types to apply.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Mutation {
        /// Set a cell's value.
        #[prost(message, tag = "1")]
        SetCell(SetCell),
        /// Deletes cells from a column.
        #[prost(message, tag = "2")]
        DeleteFromColumn(DeleteFromColumn),
        /// Deletes cells from a column family.
        #[prost(message, tag = "3")]
        DeleteFromFamily(DeleteFromFamily),
        /// Deletes cells from the entire row.
        #[prost(message, tag = "4")]
        DeleteFromRow(DeleteFromRow),
    }
}
/// Specifies an atomic read/modify/write operation on the latest value of the
/// specified column.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadModifyWriteRule {
    /// The name of the family to which the read/modify/write should be applied.
    /// Must match `\[-_.a-zA-Z0-9\]+`
    #[prost(string, tag = "1")]
    pub family_name: ::prost::alloc::string::String,
    /// The qualifier of the column to which the read/modify/write should be
    /// applied.
    /// Can be any byte string, including the empty string.
    #[prost(bytes = "vec", tag = "2")]
    pub column_qualifier: ::prost::alloc::vec::Vec<u8>,
    /// The rule used to determine the column's new latest value from its current
    /// latest value.
    #[prost(oneof = "read_modify_write_rule::Rule", tags = "3, 4")]
    pub rule: ::core::option::Option<read_modify_write_rule::Rule>,
}
/// Nested message and enum types in `ReadModifyWriteRule`.
pub mod read_modify_write_rule {
    /// The rule used to determine the column's new latest value from its current
    /// latest value.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Rule {
        /// Rule specifying that `append_value` be appended to the existing value.
        /// If the targeted cell is unset, it will be treated as containing the
        /// empty string.
        #[prost(bytes, tag = "3")]
        AppendValue(::prost::alloc::vec::Vec<u8>),
        /// Rule specifying that `increment_amount` be added to the existing value.
        /// If the targeted cell is unset, it will be treated as containing a zero.
        /// Otherwise, the targeted cell must contain an 8-byte value (interpreted
        /// as a 64-bit big-endian signed integer), or the entire request will fail.
        #[prost(int64, tag = "4")]
        IncrementAmount(i64),
    }
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// A partition of a change stream.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPartition {
    /// The row range covered by this partition and is specified by
    /// [`start_key_closed`, `end_key_open`).
    #[prost(message, optional, tag = "1")]
    pub row_range: ::core::option::Option<RowRange>,
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// The information required to continue reading the data from multiple
/// `StreamPartitions` from where a previous read left off.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamContinuationTokens {
    /// List of continuation tokens.
    #[prost(message, repeated, tag = "1")]
    pub tokens: ::prost::alloc::vec::Vec<StreamContinuationToken>,
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// The information required to continue reading the data from a
/// `StreamPartition` from where a previous read left off.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamContinuationToken {
    /// The partition that this token applies to.
    #[prost(message, optional, tag = "1")]
    pub partition: ::core::option::Option<StreamPartition>,
    /// An encoded position in the stream to restart reading from.
    #[prost(string, tag = "2")]
    pub token: ::prost::alloc::string::String,
}
/// ReadIterationStats captures information about the iteration of rows or cells
/// over the course of a read, e.g. how many results were scanned in a read
/// operation versus the results returned.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadIterationStats {
    /// The rows seen (scanned) as part of the request. This includes the count of
    /// rows returned, as captured below.
    #[prost(int64, tag = "1")]
    pub rows_seen_count: i64,
    /// The rows returned as part of the request.
    #[prost(int64, tag = "2")]
    pub rows_returned_count: i64,
    /// The cells seen (scanned) as part of the request. This includes the count of
    /// cells returned, as captured below.
    #[prost(int64, tag = "3")]
    pub cells_seen_count: i64,
    /// The cells returned as part of the request.
    #[prost(int64, tag = "4")]
    pub cells_returned_count: i64,
}
/// RequestLatencyStats provides a measurement of the latency of the request as
/// it interacts with different systems over its lifetime, e.g. how long the
/// request took to execute within a frontend server.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RequestLatencyStats {
    /// The latency measured by the frontend server handling this request, from
    /// when the request was received, to when this value is sent back in the
    /// response. For more context on the component that is measuring this latency,
    /// see: <https://cloud.google.com/bigtable/docs/overview>
    ///
    /// Note: This value may be slightly shorter than the value reported into
    /// aggregate latency metrics in Monitoring for this request
    /// (<https://cloud.google.com/bigtable/docs/monitoring-instance>) as this value
    /// needs to be sent in the response before the latency measurement including
    /// that transmission is finalized.
    ///
    /// Note: This value includes the end-to-end latency of contacting nodes in
    /// the targeted cluster, e.g. measuring from when the first byte arrives at
    /// the frontend server, to when this value is sent back as the last value in
    /// the response, including any latency incurred by contacting nodes, waiting
    /// for results from nodes, and finally sending results from nodes back to the
    /// caller.
    #[prost(message, optional, tag = "1")]
    pub frontend_server_latency: ::core::option::Option<::prost_types::Duration>,
}
/// FullReadStatsView captures all known information about a read.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FullReadStatsView {
    /// Iteration stats describe how efficient the read is, e.g. comparing
    /// rows seen vs. rows returned or cells seen vs cells returned can provide an
    /// indication of read efficiency (the higher the ratio of seen to retuned the
    /// better).
    #[prost(message, optional, tag = "1")]
    pub read_iteration_stats: ::core::option::Option<ReadIterationStats>,
    /// Request latency stats describe the time taken to complete a request, from
    /// the server side.
    #[prost(message, optional, tag = "2")]
    pub request_latency_stats: ::core::option::Option<RequestLatencyStats>,
}
/// RequestStats is the container for additional information pertaining to a
/// single request, helpful for evaluating the performance of the sent request.
/// Currently, there are the following supported methods:
///    * google.bigtable.v2.ReadRows
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RequestStats {
    /// Information pertaining to each request type received. The type is chosen
    /// based on the requested view.
    ///
    /// See the messages above for additional context.
    #[prost(oneof = "request_stats::StatsView", tags = "1")]
    pub stats_view: ::core::option::Option<request_stats::StatsView>,
}
/// Nested message and enum types in `RequestStats`.
pub mod request_stats {
    /// Information pertaining to each request type received. The type is chosen
    /// based on the requested view.
    ///
    /// See the messages above for additional context.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StatsView {
        /// Available with the ReadRowsRequest.RequestStatsView.REQUEST_STATS_FULL
        /// view, see package google.bigtable.v2.
        #[prost(message, tag = "1")]
        FullReadStatsView(super::FullReadStatsView),
    }
}
/// Request message for Bigtable.ReadRows.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadRowsRequest {
    /// Required. The unique name of the table from which to read.
    /// Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "5")]
    pub app_profile_id: ::prost::alloc::string::String,
    /// The row keys and/or ranges to read sequentially. If not specified, reads
    /// from all rows.
    #[prost(message, optional, tag = "2")]
    pub rows: ::core::option::Option<RowSet>,
    /// The filter to apply to the contents of the specified row(s). If unset,
    /// reads the entirety of each row.
    #[prost(message, optional, tag = "3")]
    pub filter: ::core::option::Option<RowFilter>,
    /// The read will stop after committing to N rows' worth of results. The
    /// default (zero) is to return all results.
    #[prost(int64, tag = "4")]
    pub rows_limit: i64,
    /// The view into RequestStats, as described above.
    #[prost(enumeration = "read_rows_request::RequestStatsView", tag = "6")]
    pub request_stats_view: i32,
    /// Experimental API - Please note that this API is currently experimental
    /// and can change in the future.
    ///
    /// Return rows in lexiographical descending order of the row keys. The row
    /// contents will not be affected by this flag.
    ///
    /// Example result set:
    ///```ignore
    ///      [
    ///        {key: "k2", "f:col1": "v1", "f:col2": "v1"},
    ///        {key: "k1", "f:col1": "v2", "f:col2": "v2"}
    ///      ]
    #[prost(bool, tag = "7")]
    pub reversed: bool,
}
/// Nested message and enum types in `ReadRowsRequest`.
pub mod read_rows_request {
    /// The desired view into RequestStats that should be returned in the response.
    ///
    /// See also: RequestStats message.
    #[derive(
        Clone,
        Copy,
        Debug,
        PartialEq,
        Eq,
        Hash,
        PartialOrd,
        Ord,
        ::prost::Enumeration
    )]
    #[repr(i32)]
    pub enum RequestStatsView {
        /// The default / unset value. The API will default to the NONE option below.
        Unspecified = 0,
        /// Do not include any RequestStats in the response. This will leave the
        /// RequestStats embedded message unset in the response.
        RequestStatsNone = 1,
        /// Include the full set of available RequestStats in the response,
        /// applicable to this read.
        RequestStatsFull = 2,
    }
    impl RequestStatsView {
        /// String value of the enum field names used in the ProtoBuf definition.
        ///
        /// The values are not transformed in any way and thus are considered stable
        /// (if the ProtoBuf definition does not change) and safe for programmatic use.
        pub fn as_str_name(&self) -> &'static str {
            match self {
                RequestStatsView::Unspecified => "REQUEST_STATS_VIEW_UNSPECIFIED",
                RequestStatsView::RequestStatsNone => "REQUEST_STATS_NONE",
                RequestStatsView::RequestStatsFull => "REQUEST_STATS_FULL",
            }
        }
        /// Creates an enum from field names used in the ProtoBuf definition.
        pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
            match value {
                "REQUEST_STATS_VIEW_UNSPECIFIED" => Some(Self::Unspecified),
                "REQUEST_STATS_NONE" => Some(Self::RequestStatsNone),
                "REQUEST_STATS_FULL" => Some(Self::RequestStatsFull),
                _ => None,
            }
        }
    }
}
/// Response message for Bigtable.ReadRows.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadRowsResponse {
    /// A collection of a row's contents as part of the read request.
    #[prost(message, repeated, tag = "1")]
    pub chunks: ::prost::alloc::vec::Vec<read_rows_response::CellChunk>,
    /// Optionally the server might return the row key of the last row it
    /// has scanned.  The client can use this to construct a more
    /// efficient retry request if needed: any row keys or portions of
    /// ranges less than this row key can be dropped from the request.
    /// This is primarily useful for cases where the server has read a
    /// lot of data that was filtered out since the last committed row
    /// key, allowing the client to skip that work on a retry.
    #[prost(bytes = "vec", tag = "2")]
    pub last_scanned_row_key: ::prost::alloc::vec::Vec<u8>,
    ///
    /// If requested, provide enhanced query performance statistics. The semantics
    /// dictate:
    ///    * request_stats is empty on every (streamed) response, except
    ///    * request_stats has non-empty information after all chunks have been
    ///      streamed, where the ReadRowsResponse message only contains
    ///      request_stats.
    ///        * For example, if a read request would have returned an empty
    ///          response instead a single ReadRowsResponse is streamed with empty
    ///          chunks and request_stats filled.
    ///
    /// Visually, response messages will stream as follows:
    ///     ... -> {chunks: \[...\]} -> {chunks: [], request_stats: {...}}
    ///    \______________________/  \________________________________/
    ///        Primary response         Trailer of RequestStats info
    ///
    /// Or if the read did not return any values:
    ///    {chunks: [], request_stats: {...}}
    ///    \________________________________/
    ///       Trailer of RequestStats info
    #[prost(message, optional, tag = "3")]
    pub request_stats: ::core::option::Option<RequestStats>,
}
/// Nested message and enum types in `ReadRowsResponse`.
pub mod read_rows_response {
    /// Specifies a piece of a row's contents returned as part of the read
    /// response stream.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct CellChunk {
        /// The row key for this chunk of data.  If the row key is empty,
        /// this CellChunk is a continuation of the same row as the previous
        /// CellChunk in the response stream, even if that CellChunk was in a
        /// previous ReadRowsResponse message.
        #[prost(bytes = "vec", tag = "1")]
        pub row_key: ::prost::alloc::vec::Vec<u8>,
        /// The column family name for this chunk of data.  If this message
        /// is not present this CellChunk is a continuation of the same column
        /// family as the previous CellChunk.  The empty string can occur as a
        /// column family name in a response so clients must check
        /// explicitly for the presence of this message, not just for
        /// `family_name.value` being non-empty.
        #[prost(message, optional, tag = "2")]
        pub family_name: ::core::option::Option<::prost::alloc::string::String>,
        /// The column qualifier for this chunk of data.  If this message
        /// is not present, this CellChunk is a continuation of the same column
        /// as the previous CellChunk.  Column qualifiers may be empty so
        /// clients must check for the presence of this message, not just
        /// for `qualifier.value` being non-empty.
        #[prost(message, optional, tag = "3")]
        pub qualifier: ::core::option::Option<::prost::alloc::vec::Vec<u8>>,
        /// The cell's stored timestamp, which also uniquely identifies it
        /// within its column.  Values are always expressed in
        /// microseconds, but individual tables may set a coarser
        /// granularity to further restrict the allowed values. For
        /// example, a table which specifies millisecond granularity will
        /// only allow values of `timestamp_micros` which are multiples of
        /// 1000.  Timestamps are only set in the first CellChunk per cell
        /// (for cells split into multiple chunks).
        #[prost(int64, tag = "4")]
        pub timestamp_micros: i64,
        /// Labels applied to the cell by a
        /// \[RowFilter][google.bigtable.v2.RowFilter\].  Labels are only set
        /// on the first CellChunk per cell.
        #[prost(string, repeated, tag = "5")]
        pub labels: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
        /// The value stored in the cell.  Cell values can be split across
        /// multiple CellChunks.  In that case only the value field will be
        /// set in CellChunks after the first: the timestamp and labels
        /// will only be present in the first CellChunk, even if the first
        /// CellChunk came in a previous ReadRowsResponse.
        #[prost(bytes = "vec", tag = "6")]
        pub value: ::prost::alloc::vec::Vec<u8>,
        /// If this CellChunk is part of a chunked cell value and this is
        /// not the final chunk of that cell, value_size will be set to the
        /// total length of the cell value.  The client can use this size
        /// to pre-allocate memory to hold the full cell value.
        #[prost(int32, tag = "7")]
        pub value_size: i32,
        /// Signals to the client concerning previous CellChunks received.
        #[prost(oneof = "cell_chunk::RowStatus", tags = "8, 9")]
        pub row_status: ::core::option::Option<cell_chunk::RowStatus>,
    }
    /// Nested message and enum types in `CellChunk`.
    pub mod cell_chunk {
        /// Signals to the client concerning previous CellChunks received.
        #[allow(clippy::derive_partial_eq_without_eq)]
        #[derive(Clone, PartialEq, ::prost::Oneof)]
        pub enum RowStatus {
            /// Indicates that the client should drop all previous chunks for
            /// `row_key`, as it will be re-read from the beginning.
            #[prost(bool, tag = "8")]
            ResetRow(bool),
            /// Indicates that the client can safely process all previous chunks for
            /// `row_key`, as its data has been fully read.
            #[prost(bool, tag = "9")]
            CommitRow(bool),
        }
    }
}
/// Request message for Bigtable.SampleRowKeys.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SampleRowKeysRequest {
    /// Required. The unique name of the table from which to sample row keys.
    /// Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "2")]
    pub app_profile_id: ::prost::alloc::string::String,
}
/// Response message for Bigtable.SampleRowKeys.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SampleRowKeysResponse {
    /// Sorted streamed sequence of sample row keys in the table. The table might
    /// have contents before the first row key in the list and after the last one,
    /// but a key containing the empty string indicates "end of table" and will be
    /// the last response given, if present.
    /// Note that row keys in this list may not have ever been written to or read
    /// from, and users should therefore not make any assumptions about the row key
    /// structure that are specific to their use case.
    #[prost(bytes = "vec", tag = "1")]
    pub row_key: ::prost::alloc::vec::Vec<u8>,
    /// Approximate total storage space used by all rows in the table which precede
    /// `row_key`. Buffering the contents of all rows between two subsequent
    /// samples would require space roughly equal to the difference in their
    /// `offset_bytes` fields.
    #[prost(int64, tag = "2")]
    pub offset_bytes: i64,
}
/// Request message for Bigtable.MutateRow.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MutateRowRequest {
    /// Required. The unique name of the table to which the mutation should be
    /// applied. Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "4")]
    pub app_profile_id: ::prost::alloc::string::String,
    /// Required. The key of the row to which the mutation should be applied.
    #[prost(bytes = "vec", tag = "2")]
    pub row_key: ::prost::alloc::vec::Vec<u8>,
    /// Required. Changes to be atomically applied to the specified row. Entries
    /// are applied in order, meaning that earlier mutations can be masked by later
    /// ones. Must contain at least one entry and at most 100000.
    #[prost(message, repeated, tag = "3")]
    pub mutations: ::prost::alloc::vec::Vec<Mutation>,
}
/// Response message for Bigtable.MutateRow.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MutateRowResponse {}
/// Request message for BigtableService.MutateRows.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MutateRowsRequest {
    /// Required. The unique name of the table to which the mutations should be
    /// applied.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "3")]
    pub app_profile_id: ::prost::alloc::string::String,
    /// Required. The row keys and corresponding mutations to be applied in bulk.
    /// Each entry is applied as an atomic mutation, but the entries may be
    /// applied in arbitrary order (even between entries for the same row).
    /// At least one entry must be specified, and in total the entries can
    /// contain at most 100000 mutations.
    #[prost(message, repeated, tag = "2")]
    pub entries: ::prost::alloc::vec::Vec<mutate_rows_request::Entry>,
}
/// Nested message and enum types in `MutateRowsRequest`.
pub mod mutate_rows_request {
    /// A mutation for a given row.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Entry {
        /// The key of the row to which the `mutations` should be applied.
        #[prost(bytes = "vec", tag = "1")]
        pub row_key: ::prost::alloc::vec::Vec<u8>,
        /// Required. Changes to be atomically applied to the specified row.
        /// Mutations are applied in order, meaning that earlier mutations can be
        /// masked by later ones. You must specify at least one mutation.
        #[prost(message, repeated, tag = "2")]
        pub mutations: ::prost::alloc::vec::Vec<super::Mutation>,
    }
}
/// Response message for BigtableService.MutateRows.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MutateRowsResponse {
    /// One or more results for Entries from the batch request.
    #[prost(message, repeated, tag = "1")]
    pub entries: ::prost::alloc::vec::Vec<mutate_rows_response::Entry>,
    /// Information about how client should limit the rate (QPS). Primirily used by
    /// supported official Cloud Bigtable clients. If unset, the rate limit info is
    /// not provided by the server.
    #[prost(message, optional, tag = "3")]
    pub rate_limit_info: ::core::option::Option<RateLimitInfo>,
}
/// Nested message and enum types in `MutateRowsResponse`.
pub mod mutate_rows_response {
    /// The result of applying a passed mutation in the original request.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Entry {
        /// The index into the original request's `entries` list of the Entry
        /// for which a result is being reported.
        #[prost(int64, tag = "1")]
        pub index: i64,
        /// The result of the request Entry identified by `index`.
        /// Depending on how requests are batched during execution, it is possible
        /// for one Entry to fail due to an error with another Entry. In the event
        /// that this occurs, the same error will be reported for both entries.
        #[prost(message, optional, tag = "2")]
        pub status: ::core::option::Option<super::super::super::rpc::Status>,
    }
}
/// Information about how client should adjust the load to Bigtable.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RateLimitInfo {
    /// Time that clients should wait before adjusting the target rate again.
    /// If clients adjust rate too frequently, the impact of the previous
    /// adjustment may not have been taken into account and may
    /// over-throttle or under-throttle. If clients adjust rate too slowly, they
    /// will not be responsive to load changes on server side, and may
    /// over-throttle or under-throttle.
    #[prost(message, optional, tag = "1")]
    pub period: ::core::option::Option<::prost_types::Duration>,
    /// If it has been at least one `period` since the last load adjustment, the
    /// client should multiply the current load by this value to get the new target
    /// load. For example, if the current load is 100 and `factor` is 0.8, the new
    /// target load should be 80. After adjusting, the client should ignore
    /// `factor` until another `period` has passed.
    ///
    /// The client can measure its load using any unit that's comparable over time
    /// For example, QPS can be used as long as each request involves a similar
    /// amount of work.
    #[prost(double, tag = "2")]
    pub factor: f64,
}
/// Request message for Bigtable.CheckAndMutateRow.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CheckAndMutateRowRequest {
    /// Required. The unique name of the table to which the conditional mutation
    /// should be applied. Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "7")]
    pub app_profile_id: ::prost::alloc::string::String,
    /// Required. The key of the row to which the conditional mutation should be
    /// applied.
    #[prost(bytes = "vec", tag = "2")]
    pub row_key: ::prost::alloc::vec::Vec<u8>,
    /// The filter to be applied to the contents of the specified row. Depending
    /// on whether or not any results are yielded, either `true_mutations` or
    /// `false_mutations` will be executed. If unset, checks that the row contains
    /// any values at all.
    #[prost(message, optional, tag = "6")]
    pub predicate_filter: ::core::option::Option<RowFilter>,
    /// Changes to be atomically applied to the specified row if `predicate_filter`
    /// yields at least one cell when applied to `row_key`. Entries are applied in
    /// order, meaning that earlier mutations can be masked by later ones.
    /// Must contain at least one entry if `false_mutations` is empty, and at most
    /// 100000.
    #[prost(message, repeated, tag = "4")]
    pub true_mutations: ::prost::alloc::vec::Vec<Mutation>,
    /// Changes to be atomically applied to the specified row if `predicate_filter`
    /// does not yield any cells when applied to `row_key`. Entries are applied in
    /// order, meaning that earlier mutations can be masked by later ones.
    /// Must contain at least one entry if `true_mutations` is empty, and at most
    /// 100000.
    #[prost(message, repeated, tag = "5")]
    pub false_mutations: ::prost::alloc::vec::Vec<Mutation>,
}
/// Response message for Bigtable.CheckAndMutateRow.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CheckAndMutateRowResponse {
    /// Whether or not the request's `predicate_filter` yielded any results for
    /// the specified row.
    #[prost(bool, tag = "1")]
    pub predicate_matched: bool,
}
/// Request message for client connection keep-alive and warming.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PingAndWarmRequest {
    /// Required. The unique name of the instance to check permissions for as well
    /// as respond. Values are of the form
    /// `projects/<project>/instances/<instance>`.
    #[prost(string, tag = "1")]
    pub name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "2")]
    pub app_profile_id: ::prost::alloc::string::String,
}
/// Response message for Bigtable.PingAndWarm connection keepalive and warming.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PingAndWarmResponse {}
/// Request message for Bigtable.ReadModifyWriteRow.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadModifyWriteRowRequest {
    /// Required. The unique name of the table to which the read/modify/write rules
    /// should be applied. Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    #[prost(string, tag = "4")]
    pub app_profile_id: ::prost::alloc::string::String,
    /// Required. The key of the row to which the read/modify/write rules should be
    /// applied.
    #[prost(bytes = "vec", tag = "2")]
    pub row_key: ::prost::alloc::vec::Vec<u8>,
    /// Required. Rules specifying how the specified row's contents are to be
    /// transformed into writes. Entries are applied in order, meaning that earlier
    /// rules will affect the results of later ones.
    #[prost(message, repeated, tag = "3")]
    pub rules: ::prost::alloc::vec::Vec<ReadModifyWriteRule>,
}
/// Response message for Bigtable.ReadModifyWriteRow.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadModifyWriteRowResponse {
    /// A Row containing the new contents of all cells modified by the request.
    #[prost(message, optional, tag = "1")]
    pub row: ::core::option::Option<Row>,
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// Request message for Bigtable.GenerateInitialChangeStreamPartitions.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GenerateInitialChangeStreamPartitionsRequest {
    /// Required. The unique name of the table from which to get change stream
    /// partitions. Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    /// Change streaming must be enabled on the table.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    /// Single cluster routing must be configured on the profile.
    #[prost(string, tag = "2")]
    pub app_profile_id: ::prost::alloc::string::String,
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// Response message for Bigtable.GenerateInitialChangeStreamPartitions.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GenerateInitialChangeStreamPartitionsResponse {
    /// A partition of the change stream.
    #[prost(message, optional, tag = "1")]
    pub partition: ::core::option::Option<StreamPartition>,
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// Request message for Bigtable.ReadChangeStream.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadChangeStreamRequest {
    /// Required. The unique name of the table from which to read a change stream.
    /// Values are of the form
    /// `projects/<project>/instances/<instance>/tables/<table>`.
    /// Change streaming must be enabled on the table.
    #[prost(string, tag = "1")]
    pub table_name: ::prost::alloc::string::String,
    /// This value specifies routing for replication. If not specified, the
    /// "default" application profile will be used.
    /// Single cluster routing must be configured on the profile.
    #[prost(string, tag = "2")]
    pub app_profile_id: ::prost::alloc::string::String,
    /// The partition to read changes from.
    #[prost(message, optional, tag = "3")]
    pub partition: ::core::option::Option<StreamPartition>,
    /// If specified, OK will be returned when the stream advances beyond
    /// this time. Otherwise, changes will be continuously delivered on the stream.
    /// This value is inclusive and will be truncated to microsecond granularity.
    #[prost(message, optional, tag = "5")]
    pub end_time: ::core::option::Option<::prost_types::Timestamp>,
    /// If specified, the duration between `Heartbeat` messages on the stream.
    /// Otherwise, defaults to 5 seconds.
    #[prost(message, optional, tag = "7")]
    pub heartbeat_duration: ::core::option::Option<::prost_types::Duration>,
    /// Options for describing where we want to start reading from the stream.
    #[prost(oneof = "read_change_stream_request::StartFrom", tags = "4, 6")]
    pub start_from: ::core::option::Option<read_change_stream_request::StartFrom>,
}
/// Nested message and enum types in `ReadChangeStreamRequest`.
pub mod read_change_stream_request {
    /// Options for describing where we want to start reading from the stream.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StartFrom {
        /// Start reading the stream at the specified timestamp. This timestamp must
        /// be within the change stream retention period, less than or equal to the
        /// current time, and after change stream creation, whichever is greater.
        /// This value is inclusive and will be truncated to microsecond granularity.
        #[prost(message, tag = "4")]
        StartTime(::prost_types::Timestamp),
        /// Tokens that describe how to resume reading a stream where reading
        /// previously left off. If specified, changes will be read starting at the
        /// the position. Tokens are delivered on the stream as part of `Heartbeat`
        /// and `CloseStream` messages.
        ///
        /// If a single token is provided, the token’s partition must exactly match
        /// the request’s partition. If multiple tokens are provided, as in the case
        /// of a partition merge, the union of the token partitions must exactly
        /// cover the request’s partition. Otherwise, INVALID_ARGUMENT will be
        /// returned.
        #[prost(message, tag = "6")]
        ContinuationTokens(super::StreamContinuationTokens),
    }
}
/// NOTE: This API is intended to be used by Apache Beam BigtableIO.
/// Response message for Bigtable.ReadChangeStream.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReadChangeStreamResponse {
    /// The data or control message on the stream.
    #[prost(oneof = "read_change_stream_response::StreamRecord", tags = "1, 2, 3")]
    pub stream_record: ::core::option::Option<read_change_stream_response::StreamRecord>,
}
/// Nested message and enum types in `ReadChangeStreamResponse`.
pub mod read_change_stream_response {
    /// A partial or complete mutation.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct MutationChunk {
        /// If set, then the mutation is a `SetCell` with a chunked value across
        /// multiple messages.
        #[prost(message, optional, tag = "1")]
        pub chunk_info: ::core::option::Option<mutation_chunk::ChunkInfo>,
        /// If this is a continuation of a chunked message (`chunked_value_offset` >
        /// 0), ignore all fields except the `SetCell`'s value and merge it with
        /// the previous message by concatenating the value fields.
        #[prost(message, optional, tag = "2")]
        pub mutation: ::core::option::Option<super::Mutation>,
    }
    /// Nested message and enum types in `MutationChunk`.
    pub mod mutation_chunk {
        /// Information about the chunking of this mutation.
        /// Only `SetCell` mutations can be chunked, and all chunks for a `SetCell`
        /// will be delivered contiguously with no other mutation types interleaved.
        #[allow(clippy::derive_partial_eq_without_eq)]
        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct ChunkInfo {
            /// The total value size of all the chunks that make up the `SetCell`.
            #[prost(int32, tag = "1")]
            pub chunked_value_size: i32,
            /// The byte offset of this chunk into the total value size of the
            /// mutation.
            #[prost(int32, tag = "2")]
            pub chunked_value_offset: i32,
            /// When true, this is the last chunk of a chunked `SetCell`.
            #[prost(bool, tag = "3")]
            pub last_chunk: bool,
        }
    }
    /// A message corresponding to one or more mutations to the partition
    /// being streamed. A single logical `DataChange` message may also be split
    /// across a sequence of multiple individual messages. Messages other than
    /// the first in a sequence will only have the `type` and `chunks` fields
    /// populated, with the final message in the sequence also containing `done`
    /// set to true.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DataChange {
        /// The type of the mutation.
        #[prost(enumeration = "data_change::Type", tag = "1")]
        pub r#type: i32,
        /// The cluster where the mutation was applied.
        /// Not set when `type` is `GARBAGE_COLLECTION`.
        #[prost(string, tag = "2")]
        pub source_cluster_id: ::prost::alloc::string::String,
        /// The row key for all mutations that are part of this `DataChange`.
        /// If the `DataChange` is chunked across multiple messages, then this field
        /// will only be set for the first message.
        #[prost(bytes = "vec", tag = "3")]
        pub row_key: ::prost::alloc::vec::Vec<u8>,
        /// The timestamp at which the mutation was applied on the Bigtable server.
        #[prost(message, optional, tag = "4")]
        pub commit_timestamp: ::core::option::Option<::prost_types::Timestamp>,
        /// A value that lets stream consumers reconstruct Bigtable's
        /// conflict resolution semantics.
        /// <https://cloud.google.com/bigtable/docs/writes#conflict-resolution>
        /// In the event that the same row key, column family, column qualifier,
        /// timestamp are modified on different clusters at the same
        /// `commit_timestamp`, the mutation with the larger `tiebreaker` will be the
        /// one chosen for the eventually consistent state of the system.
        #[prost(int32, tag = "5")]
        pub tiebreaker: i32,
        /// The mutations associated with this change to the partition.
        /// May contain complete mutations or chunks of a multi-message chunked
        /// `DataChange` record.
        #[prost(message, repeated, tag = "6")]
        pub chunks: ::prost::alloc::vec::Vec<MutationChunk>,
        /// When true, indicates that the entire `DataChange` has been read
        /// and the client can safely process the message.
        #[prost(bool, tag = "8")]
        pub done: bool,
        /// An encoded position for this stream's partition to restart reading from.
        /// This token is for the StreamPartition from the request.
        #[prost(string, tag = "9")]
        pub token: ::prost::alloc::string::String,
        /// An estimate of the commit timestamp that is usually lower than or equal
        /// to any timestamp for a record that will be delivered in the future on the
        /// stream. It is possible that, under particular circumstances that a future
        /// record has a timestamp is is lower than a previously seen timestamp. For
        /// an example usage see
        /// <https://beam.apache.org/documentation/basics/#watermarks>
        #[prost(message, optional, tag = "10")]
        pub estimated_low_watermark: ::core::option::Option<::prost_types::Timestamp>,
    }
    /// Nested message and enum types in `DataChange`.
    pub mod data_change {
        /// The type of mutation.
        #[derive(
            Clone,
            Copy,
            Debug,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            ::prost::Enumeration
        )]
        #[repr(i32)]
        pub enum Type {
            /// The type is unspecified.
            Unspecified = 0,
            /// A user-initiated mutation.
            User = 1,
            /// A system-initiated mutation as part of garbage collection.
            /// <https://cloud.google.com/bigtable/docs/garbage-collection>
            GarbageCollection = 2,
            /// This is a continuation of a multi-message change.
            Continuation = 3,
        }
        impl Type {
            /// String value of the enum field names used in the ProtoBuf definition.
            ///
            /// The values are not transformed in any way and thus are considered stable
            /// (if the ProtoBuf definition does not change) and safe for programmatic use.
            pub fn as_str_name(&self) -> &'static str {
                match self {
                    Type::Unspecified => "TYPE_UNSPECIFIED",
                    Type::User => "USER",
                    Type::GarbageCollection => "GARBAGE_COLLECTION",
                    Type::Continuation => "CONTINUATION",
                }
            }
            /// Creates an enum from field names used in the ProtoBuf definition.
            pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
                match value {
                    "TYPE_UNSPECIFIED" => Some(Self::Unspecified),
                    "USER" => Some(Self::User),
                    "GARBAGE_COLLECTION" => Some(Self::GarbageCollection),
                    "CONTINUATION" => Some(Self::Continuation),
                    _ => None,
                }
            }
        }
    }
    /// A periodic message with information that can be used to checkpoint
    /// the state of a stream.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Heartbeat {
        /// A token that can be provided to a subsequent `ReadChangeStream` call
        /// to pick up reading at the current stream position.
        #[prost(message, optional, tag = "1")]
        pub continuation_token: ::core::option::Option<super::StreamContinuationToken>,
        /// An estimate of the commit timestamp that is usually lower than or equal
        /// to any timestamp for a record that will be delivered in the future on the
        /// stream. It is possible that, under particular circumstances that a future
        /// record has a timestamp is is lower than a previously seen timestamp. For
        /// an example usage see
        /// <https://beam.apache.org/documentation/basics/#watermarks>
        #[prost(message, optional, tag = "2")]
        pub estimated_low_watermark: ::core::option::Option<::prost_types::Timestamp>,
    }
    /// A message indicating that the client should stop reading from the stream.
    /// If status is OK and `continuation_tokens` & `new_partitions` are empty, the
    /// stream has finished (for example if there was an `end_time` specified).
    /// If `continuation_tokens` & `new_partitions` are present, then a change in
    /// partitioning requires the client to open a new stream for each token to
    /// resume reading. Example:
    ///                                   [B,      D) ends
    ///                                        |
    ///                                        v
    ///                new_partitions:  [A,  C) [C,  E)
    /// continuation_tokens.partitions:  [B,C) [C,D)
    ///                                   ^---^ ^---^
    ///                                   ^     ^
    ///                                   |     |
    ///                                   |     StreamContinuationToken 2
    ///                                   |
    ///                                   StreamContinuationToken 1
    /// To read the new partition [A,C), supply the continuation tokens whose
    /// ranges cover the new partition, for example ContinuationToken[A,B) &
    /// ContinuationToken[B,C).
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct CloseStream {
        /// The status of the stream.
        #[prost(message, optional, tag = "1")]
        pub status: ::core::option::Option<super::super::super::rpc::Status>,
        /// If non-empty, contains the information needed to resume reading their
        /// associated partitions.
        #[prost(message, repeated, tag = "2")]
        pub continuation_tokens: ::prost::alloc::vec::Vec<
            super::StreamContinuationToken,
        >,
        /// If non-empty, contains the new partitions to start reading from, which
        /// are related to but not necessarily identical to the partitions for the
        /// above `continuation_tokens`.
        #[prost(message, repeated, tag = "3")]
        pub new_partitions: ::prost::alloc::vec::Vec<super::StreamPartition>,
    }
    /// The data or control message on the stream.
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StreamRecord {
        /// A mutation to the partition.
        #[prost(message, tag = "1")]
        DataChange(DataChange),
        /// A periodic heartbeat message.
        #[prost(message, tag = "2")]
        Heartbeat(Heartbeat),
        /// An indication that the stream should be closed.
        #[prost(message, tag = "3")]
        CloseStream(CloseStream),
    }
}
/// Generated client implementations.
pub mod bigtable_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /// Service for reading from and writing to existing Bigtable tables.
    #[derive(Debug, Clone)]
    pub struct BigtableClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl BigtableClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> BigtableClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> BigtableClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            BigtableClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        /// Streams back the contents of all requested rows in key order, optionally
        /// applying the same Reader filter to each. Depending on their size,
        /// rows and cells may be broken up across multiple responses, but
        /// atomicity of each row will still be preserved. See the
        /// ReadRowsResponse documentation for details.
        pub async fn read_rows(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadRowsRequest>,
        ) -> std::result::Result<
                tonic::Response<tonic::codec::Streaming<super::ReadRowsResponse>>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/ReadRows",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("google.bigtable.v2.Bigtable", "ReadRows"));
            self.inner.server_streaming(req, path, codec).await
        }
        /// Returns a sample of row keys in the table. The returned row keys will
        /// delimit contiguous sections of the table of approximately equal size,
        /// which can be used to break up the data for distributed tasks like
        /// mapreduces.
        pub async fn sample_row_keys(
            &mut self,
            request: impl tonic::IntoRequest<super::SampleRowKeysRequest>,
        ) -> std::result::Result<
                tonic::Response<tonic::codec::Streaming<super::SampleRowKeysResponse>>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/SampleRowKeys",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("google.bigtable.v2.Bigtable", "SampleRowKeys"));
            self.inner.server_streaming(req, path, codec).await
        }
        /// Mutates a row atomically. Cells already present in the row are left
        /// unchanged unless explicitly changed by `mutation`.
        pub async fn mutate_row(
            &mut self,
            request: impl tonic::IntoRequest<super::MutateRowRequest>,
        ) -> std::result::Result<
                tonic::Response<super::MutateRowResponse>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/MutateRow",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("google.bigtable.v2.Bigtable", "MutateRow"));
            self.inner.unary(req, path, codec).await
        }
        /// Mutates multiple rows in a batch. Each individual row is mutated
        /// atomically as in MutateRow, but the entire batch is not executed
        /// atomically.
        pub async fn mutate_rows(
            &mut self,
            request: impl tonic::IntoRequest<super::MutateRowsRequest>,
        ) -> std::result::Result<
                tonic::Response<tonic::codec::Streaming<super::MutateRowsResponse>>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/MutateRows",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("google.bigtable.v2.Bigtable", "MutateRows"));
            self.inner.server_streaming(req, path, codec).await
        }
        /// Mutates a row atomically based on the output of a predicate Reader filter.
        pub async fn check_and_mutate_row(
            &mut self,
            request: impl tonic::IntoRequest<super::CheckAndMutateRowRequest>,
        ) -> std::result::Result<
                tonic::Response<super::CheckAndMutateRowResponse>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/CheckAndMutateRow",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("google.bigtable.v2.Bigtable", "CheckAndMutateRow"),
                );
            self.inner.unary(req, path, codec).await
        }
        /// Warm up associated instance metadata for this connection.
        /// This call is not required but may be useful for connection keep-alive.
        pub async fn ping_and_warm(
            &mut self,
            request: impl tonic::IntoRequest<super::PingAndWarmRequest>,
        ) -> std::result::Result<
                tonic::Response<super::PingAndWarmResponse>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/PingAndWarm",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("google.bigtable.v2.Bigtable", "PingAndWarm"));
            self.inner.unary(req, path, codec).await
        }
        /// Modifies a row atomically on the server. The method reads the latest
        /// existing timestamp and value from the specified columns and writes a new
        /// entry based on pre-defined read/modify/write rules. The new value for the
        /// timestamp is the greater of the existing timestamp or the current server
        /// time. The method returns the new contents of all modified cells.
        pub async fn read_modify_write_row(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadModifyWriteRowRequest>,
        ) -> std::result::Result<
                tonic::Response<super::ReadModifyWriteRowResponse>,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/ReadModifyWriteRow",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("google.bigtable.v2.Bigtable", "ReadModifyWriteRow"),
                );
            self.inner.unary(req, path, codec).await
        }
        /// NOTE: This API is intended to be used by Apache Beam BigtableIO.
        /// Returns the current list of partitions that make up the table's
        /// change stream. The union of partitions will cover the entire keyspace.
        /// Partitions can be read with `ReadChangeStream`.
        pub async fn generate_initial_change_stream_partitions(
            &mut self,
            request: impl tonic::IntoRequest<
                super::GenerateInitialChangeStreamPartitionsRequest,
            >,
        ) -> std::result::Result<
                tonic::Response<
                    tonic::codec::Streaming<
                        super::GenerateInitialChangeStreamPartitionsResponse,
                    >,
                >,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/GenerateInitialChangeStreamPartitions",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "google.bigtable.v2.Bigtable",
                        "GenerateInitialChangeStreamPartitions",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        /// NOTE: This API is intended to be used by Apache Beam BigtableIO.
        /// Reads changes from a table's change stream. Changes will
        /// reflect both user-initiated mutations and mutations that are caused by
        /// garbage collection.
        pub async fn read_change_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadChangeStreamRequest>,
        ) -> std::result::Result<
                tonic::Response<
                    tonic::codec::Streaming<super::ReadChangeStreamResponse>,
                >,
                tonic::Status,
            > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/google.bigtable.v2.Bigtable/ReadChangeStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("google.bigtable.v2.Bigtable", "ReadChangeStream"),
                );
            self.inner.server_streaming(req, path, codec).await
        }
    }
}
