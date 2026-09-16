//! A rotate-and-add sum: rotate the accumulator right one bit, add the byte, modulo 2¹⁶, over a
//! range that excises the field's own two bytes.

use layline::Checksum;

pub struct RotateAddSum;

impl Checksum for RotateAddSum {
    type Output = u16;
    type State = u16;

    fn init() -> u16 {
        0
    }

    fn update(state: u16, bytes: &[u8]) -> u16 {
        bytes.iter().fold(state, |sum, &b| sum.rotate_right(1).wrapping_add(u16::from(b)))
    }

    fn finish(state: u16) -> u16 {
        state
    }
}
