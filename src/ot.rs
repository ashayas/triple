//! Small, dependency-free text operational transform used by the shared code
//! editor. Positions are UTF-16 code units, matching JavaScript strings, so the
//! browser and the server agree on indices even with non-BMP characters.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Op {
    Retain { retain: usize },
    Insert { insert: String },
    Delete { delete: usize },
}

impl Op {
    pub fn retain(n: usize) -> Op {
        Op::Retain { retain: n }
    }
    pub fn insert(s: impl Into<String>) -> Op {
        Op::Insert { insert: s.into() }
    }
    pub fn delete(n: usize) -> Op {
        Op::Delete { delete: n }
    }

    fn is_noop(&self) -> bool {
        matches!(self, Op::Retain { retain: 0 } | Op::Delete { delete: 0 })
            || matches!(self, Op::Insert { insert } if insert.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Atom {
    Retain,
    Delete,
    Insert(String),
}

fn atoms(ops: &[Op]) -> Vec<Atom> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Retain { retain } => out.extend(std::iter::repeat_n(Atom::Retain, *retain)),
            Op::Delete { delete } => out.extend(std::iter::repeat_n(Atom::Delete, *delete)),
            Op::Insert { insert } => {
                if !insert.is_empty() {
                    out.push(Atom::Insert(insert.clone()));
                }
            }
        }
    }
    out
}

fn merge(atoms: Vec<Atom>) -> Vec<Op> {
    let mut out: Vec<Op> = Vec::new();
    for atom in atoms {
        match atom {
            Atom::Retain => match out.last_mut() {
                Some(Op::Retain { retain }) => *retain += 1,
                _ => out.push(Op::retain(1)),
            },
            Atom::Delete => match out.last_mut() {
                Some(Op::Delete { delete }) => *delete += 1,
                _ => out.push(Op::delete(1)),
            },
            Atom::Insert(s) => match out.last_mut() {
                Some(Op::Insert { insert }) => insert.push_str(&s),
                _ => out.push(Op::insert(s)),
            },
        }
    }
    out.retain(|op| !op.is_noop());
    out
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Transform two concurrent operations. Returns `(a', b')` such that
/// `apply(apply(doc, a), b') == apply(apply(doc, b), a')`.
pub fn transform(a: &[Op], b: &[Op]) -> (Vec<Op>, Vec<Op>) {
    let a_atoms = atoms(a);
    let b_atoms = atoms(b);
    let mut a_out: Vec<Atom> = Vec::with_capacity(a_atoms.len());
    let mut b_out: Vec<Atom> = Vec::with_capacity(b_atoms.len());
    let mut i = 0;
    let mut j = 0;

    while i < a_atoms.len() || j < b_atoms.len() {
        match (a_atoms.get(i), b_atoms.get(j)) {
            (Some(Atom::Insert(s)), _) => {
                b_out.extend(std::iter::repeat_n(Atom::Retain, utf16_len(s)));
                a_out.push(Atom::Insert(s.clone()));
                i += 1;
            }
            (_, Some(Atom::Insert(s))) => {
                a_out.extend(std::iter::repeat_n(Atom::Retain, utf16_len(s)));
                b_out.push(Atom::Insert(s.clone()));
                j += 1;
            }
            (Some(Atom::Retain), Some(Atom::Retain)) => {
                a_out.push(Atom::Retain);
                b_out.push(Atom::Retain);
                i += 1;
                j += 1;
            }
            (Some(Atom::Delete), Some(Atom::Delete)) => {
                i += 1;
                j += 1;
            }
            (Some(Atom::Delete), Some(Atom::Retain)) => {
                a_out.push(Atom::Delete);
                i += 1;
                j += 1;
            }
            (Some(Atom::Retain), Some(Atom::Delete)) => {
                b_out.push(Atom::Delete);
                i += 1;
                j += 1;
            }
            (Some(Atom::Retain), None) => {
                a_out.push(Atom::Retain);
                i += 1;
            }
            (Some(Atom::Delete), None) => {
                a_out.push(Atom::Delete);
                i += 1;
            }
            (None, Some(Atom::Retain)) => {
                b_out.push(Atom::Retain);
                j += 1;
            }
            (None, Some(Atom::Delete)) => {
                b_out.push(Atom::Delete);
                j += 1;
            }
            (None, None) => break,
        }
    }

    (merge(a_out), merge(b_out))
}

/// Apply an operation to a UTF-16 buffer. The operation must cover the whole
/// document (retains + deletes == current length).
pub fn apply(units: &mut Vec<u16>, ops: &[Op]) -> Result<(), String> {
    let mut out: Vec<u16> = Vec::with_capacity(units.len() + 32);
    let mut pos = 0usize;
    for op in ops {
        match op {
            Op::Retain { retain } => {
                if pos + retain > units.len() {
                    return Err(format!(
                        "retain {retain} at {pos} exceeds document length {}",
                        units.len()
                    ));
                }
                out.extend_from_slice(&units[pos..pos + retain]);
                pos += retain;
            }
            Op::Delete { delete } => {
                if pos + delete > units.len() {
                    return Err(format!(
                        "delete {delete} at {pos} exceeds document length {}",
                        units.len()
                    ));
                }
                pos += delete;
            }
            Op::Insert { insert } => out.extend(insert.encode_utf16()),
        }
    }
    if pos != units.len() {
        return Err(format!("operation covered {pos} of {} units", units.len()));
    }
    *units = out;
    Ok(())
}

/// Compute the operation that turns `old` into `new` using a common
/// prefix/suffix diff (one contiguous change).
pub fn diff(old: &str, new: &str) -> Vec<Op> {
    let a: Vec<u16> = old.encode_utf16().collect();
    let b: Vec<u16> = new.encode_utf16().collect();
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut ops = Vec::new();
    if prefix > 0 {
        ops.push(Op::retain(prefix));
    }
    if a.len() - prefix - suffix > 0 {
        ops.push(Op::delete(a.len() - prefix - suffix));
    }
    if b.len() - prefix - suffix > 0 {
        let inserted = String::from_utf16_lossy(&b[prefix..b.len() - suffix]);
        ops.push(Op::insert(inserted));
    }
    if suffix > 0 {
        ops.push(Op::retain(suffix));
    }
    ops
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyError {
    /// The client's base revision is older than our retained history.
    Stale,
    /// The client is ahead of the server (should not happen).
    Future,
    /// The operation did not match the document.
    Invalid,
}

pub struct Document {
    units: Vec<u16>,
    rev: u64,
    history: VecDeque<(u64, Vec<Op>)>,
    max_history: usize,
}

impl Document {
    pub fn new(text: &str) -> Self {
        Self {
            units: text.encode_utf16().collect(),
            rev: 0,
            history: VecDeque::new(),
            max_history: 500,
        }
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Apply a client operation based at `base_rev`, transforming it over any
    /// operations that have landed since. Returns the new revision and the
    /// transformed operation to broadcast to the other clients.
    pub fn apply(&mut self, base_rev: u64, mut ops: Vec<Op>) -> Result<(u64, Vec<Op>), ApplyError> {
        if base_rev > self.rev {
            return Err(ApplyError::Future);
        }
        let oldest = self.rev.saturating_sub(self.history.len() as u64);
        if base_rev < oldest {
            return Err(ApplyError::Stale);
        }
        for (entry_rev, entry_ops) in self.history.iter() {
            if *entry_rev >= base_rev {
                ops = transform(&ops, entry_ops).0;
            }
        }
        apply(&mut self.units, &ops).map_err(|_| ApplyError::Invalid)?;
        let applied_at = self.rev;
        self.rev += 1;
        self.history.push_back((applied_at, ops.clone()));
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
        Ok((self.rev, ops))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_str(text: &str, ops: &[Op]) -> String {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        apply(&mut units, ops).unwrap();
        String::from_utf16_lossy(&units)
    }

    #[test]
    fn diff_and_apply_roundtrip() {
        for (old, new) in [
            ("", "hello"),
            ("hello", ""),
            ("hello", "hello world"),
            ("hello world", "hello"),
            ("abc", "axc"),
            ("héllo", "héllo wörld"),
            ("😀 hi", "😀😀 hi"),
        ] {
            let ops = diff(old, new);
            assert_eq!(apply_str(old, &ops), new, "diff {old:?} -> {new:?}");
        }
    }

    #[test]
    fn transform_converges() {
        let base = "the quick brown fox";
        let n = base.encode_utf16().count();
        let cases = [
            (
                vec![Op::insert("A"), Op::retain(n)],
                vec![Op::insert("B"), Op::retain(n)],
            ),
            (
                vec![Op::retain(4), Op::delete(3), Op::retain(n - 7)],
                vec![Op::retain(2), Op::insert("XY"), Op::retain(n - 2)],
            ),
            (
                vec![Op::retain(4), Op::insert("Z"), Op::retain(n - 4)],
                vec![Op::retain(6), Op::delete(2), Op::retain(n - 8)],
            ),
            (
                vec![Op::retain(0), Op::insert("😀"), Op::retain(n)],
                vec![Op::insert("🎉"), Op::retain(n)],
            ),
        ];
        for (a, b) in cases {
            let (ap, bp) = transform(&a, &b);
            let left = apply_str(&apply_str(base, &a), &bp);
            let right = apply_str(&apply_str(base, &b), &ap);
            assert_eq!(left, right, "transform diverged for {a:?} / {b:?}");
        }
    }

    #[test]
    fn document_applies_concurrent_ops() {
        let mut doc = Document::new("hello world");
        let a = diff("hello world", "hello brave world");
        let b = diff("hello world", "hello world!");
        let (rev_a, _) = doc.apply(0, a).unwrap();
        assert_eq!(rev_a, 1);
        let (rev_b, _) = doc.apply(0, b).unwrap();
        assert_eq!(rev_b, 2);
        let text = doc.text();
        assert!(text.contains("brave"), "{text}");
        assert!(text.ends_with('!'), "{text}");
    }

    #[test]
    fn stale_revision_is_rejected() {
        let mut doc = Document::new("abc");
        doc.max_history = 2;
        for _ in 0..5 {
            let current = doc.text();
            let next = format!("{current}x");
            let ops = diff(&current, &next);
            let rev = doc.rev();
            doc.apply(rev, ops).unwrap();
        }
        let current = doc.text();
        let err = doc
            .apply(0, diff(&current, &format!("{current}y")))
            .unwrap_err();
        assert_eq!(err, ApplyError::Stale);
    }

    #[test]
    fn future_revision_is_rejected() {
        let mut doc = Document::new("abc");
        assert_eq!(
            doc.apply(9, diff("abc", "x")).unwrap_err(),
            ApplyError::Future
        );
    }
}
