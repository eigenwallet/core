//! Utilities for easily generating tables.

use comfy_table::Table;
use std::fmt::Display;

pub trait ToTable {
    fn to_table(self) -> Table;
}

trait TupleToRow {
    fn to_row(self) -> Vec<String>;
}

impl<A: Display> TupleToRow for (A,) {
    fn to_row(self) -> Vec<String> {
        vec![self.0.to_string()]
    }
}

impl<A: Display, B: Display> TupleToRow for (A, B) {
    fn to_row(self) -> Vec<String> {
        vec![self.0.to_string(), self.1.to_string()]
    }
}

impl<A: Display, B: Display, C: Display> TupleToRow for (A, B, C) {
    fn to_row(self) -> Vec<String> {
        vec![self.0.to_string(), self.1.to_string(), self.2.to_string()]
    }
}

impl<I, T: TupleToRow> ToTable for I
where
    I: IntoIterator<Item = T>,
{
    fn to_table(self) -> Table {
        let mut table = Table::new();
        for item in self {
            table.add_row(item.to_row());
        }
        table
    }
}
