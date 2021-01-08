// Copyright 2020 EinsteinDB Project Authors & WHTCORPS INC. Licensed under Apache-2.0.

// TODO: This value is chosen based on MonetDB/X100's research without our own benchmarks.
pub const BATCH_MAX_SIZE: usize = 1024;

/// Identical logical EventIdx is a special case in expression evaluation that
/// the events in physical_value are continuous and in order.
pub static IDENTICAL_LOGICAL_ROWS: [usize; BATCH_MAX_SIZE] = {
    let mut logical_rows = [0; BATCH_MAX_SIZE];
    let mut EventIdx = 0;
    while EventIdx < logical_rows.len() {
        logical_rows[EventIdx] = EventIdx;
        EventIdx += 1;
    }
    logical_rows
};

/// LogicalEvents is a replacement for `logical_rows` parameter
/// in many of the copr functions. By distinguishing identical
/// and non-identical mapping with a enum, we can directly
/// tell if a `logical_rows` contains all items in a vector,
/// and we may optimiaze many cases by using direct copy and
/// construction.
///
/// Note that `Identical` supports no more than `BATCH_MAX_SIZE`
/// events. In this way, it is always recommlightlikeed to use `get_idx`
/// instead of `as_slice` to avoid runtime error.
#[derive(Clone, Copy, Debug)]
pub enum LogicalEvents<'a> {
    Identical { size: usize },
    Ref { logical_rows: &'a [usize] },
}

impl<'a> LogicalEvents<'a> {
    pub fn new_ident(size: usize) -> Self {
        Self::Identical { size }
    }

    pub fn from_slice(logical_rows: &'a [usize]) -> Self {
        Self::Ref { logical_rows }
    }

    /// Convert `LogicalEvents` into legacy `&[usize]`.
    /// This function should only be called if you are sure
    /// that identical `logical_rows` doesn't exceed `BATCH_MAX_SIZE`.
    /// This function will be phased out after all `logical_rows`
    /// are refactored to use the new form.
    pub fn as_slice(self) -> &'a [usize] {
        match self {
            LogicalEvents::Identical { size } => {
                if size >= BATCH_MAX_SIZE {
                    panic!("construct identical logical_rows larger than batch size")
                }
                &IDENTICAL_LOGICAL_ROWS[0..size]
            }
            LogicalEvents::Ref { logical_rows } => logical_rows,
        }
    }

    #[inline]
    pub fn get_idx(&self, idx: usize) -> usize {
        match self {
            LogicalEvents::Identical { size: _ } => idx,
            LogicalEvents::Ref { logical_rows } => logical_rows[idx],
        }
    }

    pub fn is_ident(&self) -> bool {
        match self {
            LogicalEvents::Identical { size: _ } => true,
            LogicalEvents::Ref { logical_rows: _ } => false,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            LogicalEvents::Identical { size } => *size,
            LogicalEvents::Ref { logical_rows } => logical_rows.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct LogicalEventsIterator<'a> {
    logical_rows: LogicalEvents<'a>,
    idx: usize,
}

impl<'a> Iteron for LogicalEventsIterator<'a> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        let result = if self.idx < self.logical_rows.len() {
            Some(self.logical_rows.get_idx(self.idx))
        } else {
            None
        };

        self.idx += 1;

        result
    }
}

impl<'a> IntoIterator for LogicalEvents<'a> {
    type Item = usize;
    type IntoIter = LogicalEventsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        LogicalEventsIterator {
            logical_rows: self,
            idx: 0,
        }
    }
}
