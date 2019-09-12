use std::borrow::Cow;

use memchr::memchr2;

use crate::bitkmer::BitNuclKmer;
use crate::kmer::{complement, CanonicalKmers, Kmers};

/// Transform a nucleic acid sequence into its "normalized" form.
///
/// The normalized form is:
///  - only AGCTN and possibly - (for gaps)
///  - strip out any whitespace or line endings
///  - lowercase versions of these are uppercased
///  - U is converted to T (make everything a DNA sequence)
///  - some other punctuation is converted to gaps
///  - IUPAC bases may be converted to N's depending on the parameter passed in
///  - everything else is considered a N
pub fn normalize(seq: &[u8], allow_iupac: bool) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity(seq.len());
    let mut changed: bool = false;

    for n in seq.iter() {
        let (new_char, char_changed) = match (*n, allow_iupac) {
            c @ (b'A', _)
            | c @ (b'C', _)
            | c @ (b'G', _)
            | c @ (b'T', _)
            | c @ (b'N', _)
            | c @ (b'-', _) => (c.0, false),
            (b'a', _) => (b'A', true),
            (b'c', _) => (b'C', true),
            (b'g', _) => (b'G', true),
            // normalize uridine to thymine
            (b't', _) | (b'u', _) | (b'U', _) => (b'T', true),
            // normalize gaps
            (b'.', _) | (b'~', _) => (b'-', true),
            // logic for IUPAC bases (a little messy)
            c @ (b'B', true)
            | c @ (b'D', true)
            | c @ (b'H', true)
            | c @ (b'V', true)
            | c @ (b'R', true)
            | c @ (b'Y', true)
            | c @ (b'S', true)
            | c @ (b'W', true)
            | c @ (b'K', true)
            | c @ (b'M', true) => (c.0, false),
            (b'b', true) => (b'B', true),
            (b'd', true) => (b'D', true),
            (b'h', true) => (b'H', true),
            (b'v', true) => (b'V', true),
            (b'r', true) => (b'R', true),
            (b'y', true) => (b'Y', true),
            (b's', true) => (b'S', true),
            (b'w', true) => (b'W', true),
            (b'k', true) => (b'K', true),
            (b'm', true) => (b'M', true),
            // remove all whitespace and line endings
            (b' ', _) | (b'\t', _) | (b'\r', _) | (b'\n', _) => (b' ', true),
            // everything else is an N
            _ => (b'N', true),
        };
        changed = changed || char_changed;
        if new_char != b' ' {
            buf.push(new_char);
        }
    }
    if changed {
        Some(buf)
    } else {
        None
    }
}

#[test]
fn test_normalize() {
    assert_eq!(normalize(b"ACGTU", false), Some(b"ACGTT".to_vec()));
    assert_eq!(normalize(b"acgtu", false), Some(b"ACGTT".to_vec()));

    assert_eq!(normalize(b"N.N-N~N N", false), Some(b"N-N-N-NN".to_vec()));

    assert_eq!(normalize(b"BDHVRYSWKM", true), None);
    assert_eq!(normalize(b"bdhvryswkm", true), Some(b"BDHVRYSWKM".to_vec()));
    assert_eq!(
        normalize(b"BDHVRYSWKM", false),
        Some(b"NNNNNNNNNN".to_vec())
    );
    assert_eq!(
        normalize(b"bdhvryswkm", false),
        Some(b"NNNNNNNNNN".to_vec())
    );
}

/// A generic FASTX record that also abstracts over several logical operations
/// that can be performed on nucleic acid sequences.
pub trait Sequence<'a> {
    fn sequence(&'a self) -> &'a [u8];

    /// Remove newlines from the sequence; this handles `\r`, `\n`, and `\r\n`
    /// and removes internal newlines in addition to ones at the end.
    /// Primarily used for FASTA multiline records, but can also help process
    /// (the much rarer) multiline FASTQs. Always use before iteration methods
    /// below to ensure no newlines are being returned with e.g. `.kmers`.
    fn strip_returns(&'a self) -> Cow<'a, [u8]> {
        let seq = self.sequence();

        // first part is a fast check to see if we need to do any allocations
        let mut i;
        match memchr2(b'\r', b'\n', &seq) {
            Some(break_loc) => i = break_loc,
            None => return seq.into(),
        }
        // we found a newline; create a new buffer and stripping out newlines
        // and writing into it
        let mut new_buf = Vec::with_capacity(seq.len() - 1);
        new_buf.extend_from_slice(&seq[..i]);
        while i < seq.len() {
            match memchr2(b'\r', b'\n', &seq[i..]) {
                None => {
                    new_buf.extend_from_slice(&seq[i..]);
                    break;
                }
                Some(match_pos) => {
                    new_buf.extend_from_slice(&seq[i..i + match_pos]);
                    i += match_pos + 1;
                }
            }
        }
        new_buf.into()
    }

    /// Returns the reverse complement of a sequence. Biologically this is
    /// equivalent to the sequence of the strand opposite the one you pass
    /// in.
    fn reverse_complement(&'a self) -> Vec<u8> {
        self.sequence()
            .iter()
            .rev()
            .map(|n| complement(*n))
            .collect()
    }

    /// [Nucleic Acids] Normalizes the sequence. See documentation for
    /// `needletail::sequence::normalize`.
    fn normalize(&'a self, iupac: bool) -> Cow<'a, [u8]> {
        if let Some(s) = normalize(&self.sequence(), iupac) {
            s.into()
        } else {
            self.sequence().into()
        }
    }

    /// [Nucleic Acids] Returns an iterator over the sequence that skips
    /// non-ACGT bases and returns a tuple containing (position, the
    /// canonicalized kmer, if the sequence is the complement of the original).
    fn canonical_kmers(&'a self, k: u8, reverse_complement: &'a [u8]) -> CanonicalKmers<'a> {
        CanonicalKmers::new(self.sequence().as_ref(), reverse_complement, k)
    }

    /// Returns an iterator that returns a sliding window of k-sized
    /// sequences (k-mers). Does not skip whitespace or correct bases in the
    /// original sequence so `.normalize` or `.strip_returns` may be
    /// appropriate to use first.
    fn kmers(&'a self, k: u8) -> Kmers<'a> {
        Kmers::new(self.sequence().as_ref(), k)
    }

    /// Return an iterator that returns valid kmers in 4-bit form
    fn bit_kmers(&'a self, k: u8, canonical: bool) -> BitNuclKmer<'a> {
        BitNuclKmer::new(self.sequence(), k, canonical)
    }
}

impl<'a> Sequence<'a> for &'a [u8] {
    fn sequence(&'a self) -> &'a [u8] {
        &self
    }
}

impl<'a> Sequence<'a> for [u8] {
    fn sequence(&'a self) -> &'a [u8] {
        &self
    }
}

impl<'a> Sequence<'a> for Cow<'a, [u8]> {
    fn sequence(&'a self) -> &'a [u8] {
        &self
    }
}

pub trait QualitySequence<'a>: Sequence<'a> {
    fn quality(&'a self) -> &'a [u8];

    /// Given a SeqRecord and a quality cutoff, mask out low-quality bases with
    /// `N` characters.
    ///
    /// Experimental.
    fn quality_mask(&'a self, score: u8) -> Cow<'a, [u8]> {
        let qual = self.quality();
        // could maybe speed this up by doing a copy of base and then
        // iterating though qual and masking?
        let seq: Vec<u8> = self
            .sequence()
            .iter()
            .zip(qual.iter())
            .map(|(base, qual)| if *qual < score { b'N' } else { *base })
            .collect();
        seq.into()
    }
}

impl<'a> Sequence<'a> for (&'a [u8], &'a [u8]) {
    fn sequence(&'a self) -> &'a [u8] {
        &self.0
    }
}

impl<'a> QualitySequence<'a> for (&'a [u8], &'a [u8]) {
    fn quality(&'a self) -> &'a [u8] {
        &self.1
    }
}

#[test]
fn test_quality_mask() {
    let seq_rec = (&b"AGCT"[..], &b"AAA0"[..]);
    let filtered_rec = seq_rec.quality_mask(b'5');
    assert_eq!(&filtered_rec[..], &b"AGCN"[..]);
}

#[test]
fn can_kmerize() {
    // test general function
    for (i, k) in b"AGCT".kmers(1).enumerate() {
        match i {
            0 => assert_eq!(k, &b"A"[..]),
            1 => assert_eq!(k, &b"G"[..]),
            2 => assert_eq!(k, &b"C"[..]),
            3 => assert_eq!(k, &b"T"[..]),
            _ => unreachable!("Too many kmers"),
        }
    }

    // test that we handle length 2 (and don't drop Ns)
    for (i, k) in b"ACNGT".kmers(2).enumerate() {
        match i {
            0 => assert_eq!(k, &b"AC"[..]),
            1 => assert_eq!(k, &b"CN"[..]),
            2 => assert_eq!(k, &b"NG"[..]),
            3 => assert_eq!(k, &b"GT"[..]),
            _ => unreachable!("Too many kmers"),
        }
    }

    // test that the minimum length works
    for k in b"AC".kmers(2) {
        assert_eq!(k, &b"AC"[..]);
    }
}

#[test]
fn can_canonicalize() {
    // test general function
    let seq = b"AGCT";
    for (i, (_, k, is_c)) in seq
        .canonical_kmers(1, &seq.reverse_complement())
        .enumerate()
    {
        match i {
            0 => {
                assert_eq!(k, &b"A"[..]);
                assert_eq!(is_c, false);
            }
            1 => {
                assert_eq!(k, &b"C"[..]);
                assert_eq!(is_c, true);
            }
            2 => {
                assert_eq!(k, &b"C"[..]);
                assert_eq!(is_c, false);
            }
            3 => {
                assert_eq!(k, &b"A"[..]);
                assert_eq!(is_c, true);
            }
            _ => unreachable!("Too many kmers"),
        }
    }

    let seq = b"AGCTA";
    for (i, (_, k, _)) in seq
        .canonical_kmers(2, &seq.reverse_complement())
        .enumerate()
    {
        match i {
            0 => assert_eq!(k, &b"AG"[..]),
            1 => assert_eq!(k, &b"GC"[..]),
            2 => assert_eq!(k, &b"AG"[..]),
            3 => assert_eq!(k, &b"TA"[..]),
            _ => unreachable!("Too many kmers"),
        }
    }

    let seq = b"AGNTA";
    for (i, (ix, k, _)) in seq
        .canonical_kmers(2, &seq.reverse_complement())
        .enumerate()
    {
        match i {
            0 => {
                assert_eq!(ix, 0);
                assert_eq!(k, &b"AG"[..]);
            }
            1 => {
                assert_eq!(ix, 3);
                assert_eq!(k, &b"TA"[..]);
            }
            _ => unreachable!("Too many kmers"),
        }
    }
}
