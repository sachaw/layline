//! Stream framing.

use core::marker::PhantomData;
use core::ops::Range;

/// What [`FrameSpec::measure`] learned from the bytes so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Measure {
    /// The frame is this many bytes long, whether or not they have all arrived.
    Frame(usize),
    /// Not enough bytes yet. The value is a lower bound on the bytes still needed; zero counts as one.
    ///
    /// Return the largest safe bound, since `More(1)` per byte makes decoding quadratic.
    /// A spec that scans for a terminator must cap its search and return
    /// [`Reject`](Measure::Reject) past the cap. Otherwise [`DesyncCause::Unbounded`] stops it at
    /// [`MAX_FRAME`](FrameSpec::MAX_FRAME).
    More(usize),
    /// Not a frame. Resynchronise past it.
    Reject,
    /// A valid frame this build cannot measure, such as one that needs a newer catalogue.
    ///
    /// Handled like [`Reject`](Measure::Reject), but counted in [`Stats::unmeasurable`].
    Unmeasurable,
}

/// How a protocol is framed.
///
/// Implement either [`frame_len`](Self::frame_len), for a length in a fixed-size header, or
/// [`measure`](Self::measure), for a format that must be read to find its end.
///
/// [`Decoder::new`] fails to compile unless `1 <= CANDIDATE_LEN <= HEADER_LEN <= MAX_FRAME`.
pub trait FrameSpec {
    /// Bytes [`candidate`](Self::candidate) needs, such as the magic or a checked header.
    const CANDIDATE_LEN: usize;

    /// Bytes [`frame_len`](Self::frame_len) needs.
    const HEADER_LEN: usize = Self::CANDIDATE_LEN;

    /// The largest frame this protocol can send.
    const MAX_FRAME: usize;

    /// What happens to a frame that fails [`verify`](Self::verify).
    const ON_CORRUPT: CorruptPolicy;

    /// When a candidate frame is incomplete, look further on for a complete, verified frame.
    ///
    /// Without a sync word, false candidates are common: an 8-bit LRC passes one position in 256.
    /// The scan calls [`measure`](Self::measure) at each candidate, so make
    /// [`candidate`](Self::candidate) as strict as possible.
    const SCAN_PAST_INCOMPLETE: bool = false;

    /// Whether a frame may start at `bytes[0]`. `bytes` has at least
    /// [`CANDIDATE_LEN`](Self::CANDIDATE_LEN) bytes.
    fn candidate(bytes: &[u8]) -> bool;

    /// The total frame length from its first [`HEADER_LEN`](Self::HEADER_LEN) bytes, or `None` for
    /// an invalid header.
    ///
    /// Only the default [`measure`](Self::measure) calls it. A spec that implements neither panics
    /// on its first frame.
    fn frame_len(_header: &[u8]) -> Option<usize> {
        unreachable!(
            "this FrameSpec implements neither `frame_len` nor `measure`. \
             Implement `frame_len` for a length in a fixed-size header, or `measure` otherwise"
        )
    }

    /// The length of the frame at the start of `bytes`, as far as `bytes` shows.
    ///
    /// `bytes` runs from the candidate position to the end of the data received. It may be
    /// shorter than a header or longer than a frame.
    fn measure(bytes: &[u8]) -> Measure {
        if bytes.len() < Self::HEADER_LEN {
            return Measure::More(Self::HEADER_LEN - bytes.len());
        }
        match Self::frame_len(&bytes[..Self::HEADER_LEN]) {
            Some(total) => Measure::Frame(total),
            None => Measure::Reject,
        }
    }

    /// Whether a complete frame, exactly `frame_len` bytes, passes its checks.
    fn verify(frame: &[u8]) -> bool;

    /// The body's byte range within a complete frame.
    fn body(frame: &[u8]) -> Range<usize>;
}

/// What happens when a frame fails verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorruptPolicy {
    /// Treat it as not a frame. Consume nothing and resume scanning one byte later.
    Resync,
    /// Treat it as a damaged frame. Consume it and return it marked [`Integrity::Corrupt`].
    Consume,
}

/// Whether a returned frame passed verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Integrity {
    /// It passed.
    Verified,
    /// It failed. Returned only under [`CorruptPolicy::Consume`].
    Corrupt,
}

/// Why bytes were discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesyncCause {
    /// No frame start in the scanned bytes.
    NoCandidate,
    /// A rejected header, a frame shorter than [`HEADER_LEN`](FrameSpec::HEADER_LEN), or a body
    /// range outside the frame.
    BadHeader,
    /// A complete frame failed verification under [`CorruptPolicy::Resync`].
    Checksum,
    /// The header describes a frame larger than [`FrameSpec::MAX_FRAME`].
    TooLarge,
    /// [`Measure::Unmeasurable`].
    Unmeasurable,
    /// [`Measure::More`] with [`MAX_FRAME`](FrameSpec::MAX_FRAME) bytes already buffered: the spec's search has no bound.
    Unbounded,
}

/// The result of one [`parse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Parsed {
    /// A complete frame at the start of the buffer.
    Frame {
        /// The frame's length.
        consumed: usize,
        /// The body's range within the frame.
        body: Range<usize>,
        /// Whether verification passed.
        integrity: Integrity,
    },
    /// More bytes are needed.
    Incomplete {
        /// A lower bound on how many.
        need: usize,
    },
    /// The start of the buffer is not a frame.
    Desync {
        /// Bytes to discard from the front.
        discard: usize,
        /// Why.
        cause: DesyncCause,
    },
}

/// Try to frame one message at the start of `buf`.
#[must_use]
pub fn parse<S: FrameSpec>(buf: &[u8]) -> Parsed {
    let mut i = 0;
    let found = loop {
        if i + S::CANDIDATE_LEN > buf.len() {
            break None;
        }
        if S::candidate(&buf[i..]) {
            break Some(i);
        }
        i += 1;
    };
    match found {
        Some(0) => {}
        Some(i) => {
            return Parsed::Desync { discard: i, cause: DesyncCause::NoCandidate };
        }
        None if i == 0 => {
            return Parsed::Incomplete { need: S::CANDIDATE_LEN - buf.len() };
        }
        None => {
            return Parsed::Desync { discard: i, cause: DesyncCause::NoCandidate };
        }
    }

    let total = match S::measure(buf) {
        Measure::Frame(total) => total,
        Measure::Reject => {
            return Parsed::Desync { discard: 1, cause: DesyncCause::BadHeader };
        }
        Measure::Unmeasurable => {
            return Parsed::Desync { discard: 1, cause: DesyncCause::Unmeasurable };
        }
        Measure::More(_) if buf.len() >= S::MAX_FRAME => {
            return Parsed::Desync { discard: 1, cause: DesyncCause::Unbounded };
        }
        Measure::More(need) => {
            if S::SCAN_PAST_INCOMPLETE
                && let Some(at) = complete_frame_beyond::<S>(buf)
            {
                return Parsed::Desync { discard: at, cause: DesyncCause::NoCandidate };
            }
            return Parsed::Incomplete { need: need.max(1) };
        }
    };
    if total > S::MAX_FRAME {
        return Parsed::Desync { discard: 1, cause: DesyncCause::TooLarge };
    }
    if total < S::HEADER_LEN {
        return Parsed::Desync { discard: 1, cause: DesyncCause::BadHeader };
    }
    if buf.len() < total {
        if S::SCAN_PAST_INCOMPLETE
            && let Some(at) = complete_frame_beyond::<S>(buf)
        {
            return Parsed::Desync { discard: at, cause: DesyncCause::NoCandidate };
        }
        return Parsed::Incomplete { need: total - buf.len() };
    }

    let frame = &buf[..total];
    let body = S::body(frame);
    if body.start > body.end || body.end > total {
        return Parsed::Desync { discard: 1, cause: DesyncCause::BadHeader };
    }
    if S::verify(frame) {
        Parsed::Frame { consumed: total, body, integrity: Integrity::Verified }
    } else {
        match S::ON_CORRUPT {
            CorruptPolicy::Resync => Parsed::Desync { discard: 1, cause: DesyncCause::Checksum },
            CorruptPolicy::Consume => {
                Parsed::Frame { consumed: total, body, integrity: Integrity::Corrupt }
            }
        }
    }
}

fn complete_frame_beyond<S: FrameSpec>(buf: &[u8]) -> Option<usize> {
    let mut i = 1;
    // Bound by `CANDIDATE_LEN`, not `HEADER_LEN`: `measure` asks for any further bytes itself.
    while i + S::CANDIDATE_LEN <= buf.len() {
        if S::candidate(&buf[i..])
            && let Measure::Frame(total) = S::measure(&buf[i..])
            && (S::HEADER_LEN..=S::MAX_FRAME).contains(&total)
            && i + total <= buf.len()
            && S::verify(&buf[i..i + total])
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Link statistics since a [`Decoder`] was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Stats {
    /// Frames returned, verified or corrupt.
    pub frames: u64,
    /// Bytes dropped outside any returned frame.
    pub bytes_discarded: u64,
    /// Failed verifications, under either [`CorruptPolicy`].
    pub checksum_errors: u64,
    /// Bytes passed to [`Decoder::push`] that did not fit.
    pub bytes_refused: u64,
    /// Frames the spec could not measure: [`Measure::Unmeasurable`].
    pub unmeasurable: u64,
}

/// A frame returned by [`Decoder::pop`], borrowed from the decoder's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Popped<'a> {
    /// The whole frame, header included.
    pub frame: &'a [u8],
    /// The body bytes.
    pub body: &'a [u8],
    /// Whether verification passed.
    pub integrity: Integrity,
}

/// A stream decoder for one [`FrameSpec`], with an `N`-byte buffer.
///
/// `N` must be at least `S::MAX_FRAME`. `Send`, `Sync`, `Clone` and `Debug` for any `S`.
pub struct Decoder<S: FrameSpec, const N: usize> {
    buf: [u8; N],
    len: usize,
    /// Length of the last returned frame, removed on the next call.
    spent: usize,
    stats: Stats,
    _spec: PhantomData<fn() -> S>,
}

impl<S: FrameSpec, const N: usize> Default for Decoder<S, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: FrameSpec, const N: usize> Clone for Decoder<S, N> {
    fn clone(&self) -> Self {
        Self {
            buf: self.buf,
            len: self.len,
            spent: self.spent,
            stats: self.stats,
            _spec: PhantomData,
        }
    }
}

impl<S: FrameSpec, const N: usize> core::fmt::Debug for Decoder<S, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Decoder")
            .field("spec", &core::any::type_name::<S>())
            .field("capacity", &N)
            .field("buffered", &&self.buf[self.spent..self.len])
            .field("spent", &self.spent)
            .field("stats", &self.stats)
            .finish()
    }
}

impl<S: FrameSpec, const N: usize> Decoder<S, N> {
    /// An empty decoder.
    #[must_use]
    pub fn new() -> Self {
        const {
            assert!(S::CANDIDATE_LEN >= 1, "a FrameSpec's CANDIDATE_LEN is at least one byte");
            assert!(
                S::CANDIDATE_LEN <= S::HEADER_LEN,
                "a FrameSpec's CANDIDATE_LEN exceeds its HEADER_LEN"
            );
            assert!(
                S::HEADER_LEN <= S::MAX_FRAME,
                "a FrameSpec's HEADER_LEN exceeds its MAX_FRAME"
            );
            assert!(
                S::MAX_FRAME <= N,
                "the decoder's buffer is smaller than the protocol's largest frame"
            );
        }
        Self { buf: [0; N], len: 0, spent: 0, stats: Stats::default(), _spec: PhantomData }
    }

    /// Link statistics since this decoder was created.
    #[must_use]
    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// Append as many bytes as fit, and return how many were taken.
    pub fn push(&mut self, bytes: &[u8]) -> usize {
        self.release();
        let take = (N - self.len).min(bytes.len());
        self.buf[self.len..self.len + take].copy_from_slice(&bytes[..take]);
        self.len += take;
        self.stats.bytes_refused =
            self.stats.bytes_refused.saturating_add((bytes.len() - take) as u64);
        take
    }

    /// The next frame, if the buffer has one.
    #[must_use]
    pub fn pop(&mut self) -> Option<Popped<'_>> {
        self.release();
        loop {
            match parse::<S>(&self.buf[..self.len]) {
                Parsed::Desync { discard, cause } => {
                    let n = discard.max(1).min(self.len);
                    self.stats.bytes_discarded =
                        self.stats.bytes_discarded.saturating_add(n as u64);
                    match cause {
                        DesyncCause::Checksum => {
                            self.stats.checksum_errors =
                                self.stats.checksum_errors.saturating_add(1);
                        }
                        DesyncCause::Unmeasurable => {
                            self.stats.unmeasurable = self.stats.unmeasurable.saturating_add(1);
                        }
                        // `Unbounded` is a bug in the spec, not a link error.
                        DesyncCause::NoCandidate
                        | DesyncCause::BadHeader
                        | DesyncCause::TooLarge
                        | DesyncCause::Unbounded => {}
                    }
                    self.consume(n);
                }
                Parsed::Incomplete { .. } => return None,
                Parsed::Frame { consumed, body, integrity } => {
                    self.spent = consumed;
                    self.stats.frames = self.stats.frames.saturating_add(1);
                    if integrity == Integrity::Corrupt {
                        self.stats.checksum_errors = self.stats.checksum_errors.saturating_add(1);
                    }
                    return Some(Popped {
                        frame: &self.buf[..consumed],
                        body: &self.buf[body],
                        integrity,
                    });
                }
            }
        }
    }

    fn release(&mut self) {
        if self.spent > 0 {
            self.consume(self.spent);
            self.spent = 0;
        }
    }

    fn consume(&mut self, n: usize) {
        self.buf.copy_within(n..self.len, 0);
        self.len -= n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte 1 is the frame length. The body follows the two-byte header.
    struct Stated;

    impl FrameSpec for Stated {
        const CANDIDATE_LEN: usize = 2;
        const MAX_FRAME: usize = 8;
        const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;

        fn candidate(bytes: &[u8]) -> bool {
            bytes[0] == 0xAA
        }

        fn frame_len(header: &[u8]) -> Option<usize> {
            Some(usize::from(header[1]))
        }

        fn verify(_: &[u8]) -> bool {
            true
        }

        fn body(frame: &[u8]) -> Range<usize> {
            2..frame.len()
        }
    }

    /// A body range that runs past its frame.
    struct Overreach;

    impl FrameSpec for Overreach {
        const CANDIDATE_LEN: usize = 1;
        const MAX_FRAME: usize = 8;
        const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;

        fn candidate(bytes: &[u8]) -> bool {
            bytes[0] == 0xAA
        }

        fn frame_len(_: &[u8]) -> Option<usize> {
            Some(2)
        }

        fn verify(_: &[u8]) -> bool {
            true
        }

        fn body(frame: &[u8]) -> Range<usize> {
            0..frame.len() + 1
        }
    }

    const BAD_HEADER: Parsed = Parsed::Desync { discard: 1, cause: DesyncCause::BadHeader };

    #[test]
    fn a_frame_shorter_than_its_candidate_is_a_bad_header() {
        for stated in [0u8, 1] {
            assert_eq!(parse::<Stated>(&[0xAA, stated, 0, 0]), BAD_HEADER, "stated {stated}");
        }

        let mut d = Decoder::<Stated, 8>::new();
        d.push(&[0xAA, 0, 0xAA, 4, 9, 9]);
        assert_eq!(d.pop().expect("the second candidate is a frame").frame, &[0xAA, 4, 9, 9]);
        assert!(d.pop().is_none(), "nothing is returned twice");
    }

    /// A three-byte header whose length byte can be less than the header.
    struct Headed;

    impl FrameSpec for Headed {
        const CANDIDATE_LEN: usize = 1;
        const HEADER_LEN: usize = 3;
        const MAX_FRAME: usize = 16;
        const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;
        const SCAN_PAST_INCOMPLETE: bool = true;

        fn candidate(bytes: &[u8]) -> bool {
            bytes[0] == 0xAA
        }

        fn frame_len(header: &[u8]) -> Option<usize> {
            Some(usize::from(header[1]))
        }

        fn verify(frame: &[u8]) -> bool {
            frame[2] == 0
        }

        fn body(frame: &[u8]) -> Range<usize> {
            3..frame.len()
        }
    }

    #[test]
    fn a_frame_shorter_than_its_header_is_a_bad_header_and_never_verified() {
        assert_eq!(parse::<Headed>(&[0xAA, 2, 0, 0]), BAD_HEADER);
        let mut d = Decoder::<Headed, 16>::new();
        d.push(&[0xAA, 2, 0, 0xAA, 3, 0]);
        assert_eq!(d.pop().expect("the second candidate is a frame").frame, &[0xAA, 3, 0]);

        assert_eq!(parse::<Headed>(&[0xAA, 10, 0, 0xAA, 0, 0]), Parsed::Incomplete { need: 4 });
    }

    #[test]
    fn a_body_outside_its_frame_is_a_bad_header() {
        assert_eq!(parse::<Overreach>(&[0xAA, 0]), BAD_HEADER);
        let mut d = Decoder::<Overreach, 8>::new();
        d.push(&[0xAA, 0]);
        assert!(d.pop().is_none());
    }
}
