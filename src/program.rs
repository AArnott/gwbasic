//! Numbered program lines. Each line is stored tokenized. `get` returns the
//! listing text, which is what `LIST` and `EDIT` show.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
struct Line {
    tokens: Vec<u8>,
    text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    lines: BTreeMap<u16, Line>,
    pub last: Option<u16>,
    protected: bool,
}

impl Program {
    pub fn insert(&mut self, number: u16, tokens: Vec<u8>, text: String) {
        self.lines.insert(number, Line { tokens, text });
        self.last = Some(number);
    }

    pub fn delete_line(&mut self, number: u16) {
        self.lines.remove(&number);
        self.last = Some(number);
    }

    pub fn delete_range(&mut self, start: Option<u16>, end: Option<u16>) {
        let start = start.unwrap_or(0);
        let end = end.unwrap_or(65529);
        self.lines.retain(|&n, _| n < start || n > end);
        self.last = Some(end.min(65529));
    }

    pub fn get(&self, number: u16) -> Option<&str> {
        self.lines.get(&number).map(|line| line.text.as_str())
    }

    pub fn code(&self, number: u16) -> Option<&[u8]> {
        self.lines.get(&number).map(|line| line.tokens.as_slice())
    }

    pub fn contains(&self, number: u16) -> bool {
        self.lines.contains_key(&number)
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.last = None;
        self.protected = false;
    }

    pub fn is_protected(&self) -> bool {
        self.protected
    }

    pub fn set_protected(&mut self, protected: bool) {
        self.protected = protected;
    }

    pub fn line_pairs(&self) -> Vec<(u16, Vec<u8>)> {
        self.lines
            .iter()
            .map(|(&n, line)| (n, line.tokens.clone()))
            .collect()
    }

    pub fn listing(&self) -> Vec<String> {
        self.lines.values().map(|line| line.text.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn first_from(&self, number: u16) -> Option<u16> {
        self.lines.range(number..).next().map(|(&n, _)| n)
    }

    pub fn next_after(&self, number: u16) -> Option<u16> {
        self.lines.range((number + 1)..).next().map(|(&n, _)| n)
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (u16, &str)> {
        self.lines.iter().map(|(&n, line)| (n, line.text.as_str()))
    }

    pub fn in_range(&self, start: Option<u16>, end: Option<u16>) -> Vec<(u16, &str)> {
        let start = start.unwrap_or(0);
        let end = end.unwrap_or(65529);
        self.lines
            .range(start..=end)
            .map(|(&n, line)| (n, line.text.as_str()))
            .collect()
    }

    pub fn iter_code(&self) -> impl Iterator<Item = (u16, &[u8])> {
        self.lines
            .iter()
            .map(|(&n, line)| (n, line.tokens.as_slice()))
    }

    /// Bytes the line occupies: next-line pointer, line number, tokens, and the
    /// terminating zero that `tokens` already includes.
    pub fn stored_bytes(&self) -> usize {
        self.lines.values().map(|line| line.tokens.len() + 4).sum()
    }
}
