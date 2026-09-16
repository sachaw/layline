//! A deterministic splitmix64 PRNG.

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn byte(&mut self) -> u8 {
        self.next() as u8
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for b in buf {
            *b = self.byte();
        }
    }

    /// A value in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Feed `stream` to a decoder in random-sized chunks and collect every popped `(body, integrity)` pair.
#[macro_export]
macro_rules! chunked_feed {
    ($decoder:expr, $stream:expr, $rng:expr) => {{
        let mut collected: Vec<(Vec<u8>, layline_core::frame::Integrity)> = Vec::new();
        let mut offset = 0;
        while offset < $stream.len() {
            let chunk = ($rng.below(17) + 1).min($stream.len() - offset);
            let mut given = 0;
            while given < chunk {
                let taken = $decoder.push(&$stream[offset + given..offset + chunk]);
                given += taken;
                while let Some(frame) = $decoder.pop() {
                    collected.push((frame.body.to_vec(), frame.integrity));
                }
                if taken == 0 {
                    break;
                }
            }
            offset += chunk;
        }
        while let Some(frame) = $decoder.pop() {
            collected.push((frame.body.to_vec(), frame.integrity));
        }
        collected
    }};
}
