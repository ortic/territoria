//! Tables for Territoria: typed columns, type inference, key matching and
//! aggregation.

pub mod aggregate;
pub mod coords;
pub mod date;
pub mod infer;
pub mod keys;
pub mod suggest;
pub mod table;

pub use aggregate::Aggregate;
pub use keys::{KeyIndex, MatchMode, MatchStats, Matching};
pub use table::{Column, ColumnData, ColumnKind, Table, Value};
