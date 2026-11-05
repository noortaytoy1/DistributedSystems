//! Makes messages smaller on the air, with no library (D255).
//!
//! The six PCs share one Wi-Fi cell, and what a node sends is many messages that look alike: the same
//! client, nearly the same sequence number and clock. So each datagram and each TCP frame is
//! squeezed by replacing what came earlier in it with a short reference back. The format is the one
//! LZ4 uses for a block: a token with two 4-bit lengths, the bytes that had no match, a 2-byte
//! distance back, and longer lengths as extra bytes. It needs no table on the receiving side.
//!
//! Owner: C (transport).

const MIN_MATCH: usize = 4;
const HASH_BITS: u32 = 12;
const MAX_DISTANCE: usize = 0xFFFF;
/// A length of this in a token means more follows in extra bytes.
const LONG: usize = 15;

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn write_long(out: &mut Vec<u8>, length: usize) {
    if length >= LONG {
        let mut left = length - LONG;
        while left >= 255 {
            out.push(255);
            left -= 255;
        }
        out.push(left as u8);
    }
}

fn write_sequence(out: &mut Vec<u8>, literals: &[u8], matched: Option<(usize, usize)>) {
    let extra = matched.map_or(0, |(_, length)| length - MIN_MATCH);
    out.push(((literals.len().min(LONG) as u8) << 4) | extra.min(LONG) as u8);
    write_long(out, literals.len());
    out.extend_from_slice(literals);
    if let Some((distance, _)) = matched {
        out.extend_from_slice(&(distance as u16).to_le_bytes());
        write_long(out, extra);
    }
}

/// The squeezed form of `input`, which can come out longer if nothing in it repeats.
pub fn squeeze(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2 + 16);
    // Where each 4 bytes were last seen, plus one, so that 0 means never.
    let mut seen = [0u32; 1 << HASH_BITS];
    let (mut anchor, mut at) = (0, 0);
    while at + MIN_MATCH <= input.len() {
        let here = word(input, at);
        let slot = (here.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize;
        let before = seen[slot] as usize;
        seen[slot] = (at + 1) as u32;
        if before != 0 && at - (before - 1) <= MAX_DISTANCE && word(input, before - 1) == here {
            let from = before - 1;
            let mut length = MIN_MATCH;
            while at + length < input.len() && input[from + length] == input[at + length] {
                length += 1;
            }
            write_sequence(&mut out, &input[anchor..at], Some((at - from, length)));
            at += length;
            anchor = at;
        } else {
            at += 1;
        }
    }
    write_sequence(&mut out, &input[anchor..], None);
    out
}

fn read_long(input: &[u8], at: &mut usize, start: usize) -> Option<usize> {
    let mut length = start;
    if start == LONG {
        loop {
            let byte = *input.get(*at)?;
            *at += 1;
            length = length.checked_add(usize::from(byte))?;
            if byte != 255 {
                break;
            }
        }
    }
    Some(length)
}

/// `input` as it was before [`squeeze`], or `None` if it is not valid or comes to over `limit` bytes.
/// Anyone can send a node a datagram, so this checks everything.
pub fn expand(input: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len().saturating_mul(2).min(limit));
    let mut at = 0;
    while at < input.len() {
        let token = input[at];
        at += 1;
        let literals = read_long(input, &mut at, usize::from(token >> 4))?;
        let end = at.checked_add(literals)?;
        if end > input.len() || out.len().checked_add(literals)? > limit {
            return None;
        }
        out.extend_from_slice(&input[at..end]);
        at = end;
        if at == input.len() {
            return Some(out);
        }
        let distance = usize::from(u16::from_le_bytes([*input.get(at)?, *input.get(at + 1)?]));
        at += 2;
        if distance == 0 || distance > out.len() {
            return None;
        }
        let length =
            read_long(input, &mut at, usize::from(token & 0x0F))?.checked_add(MIN_MATCH)?;
        if out.len().checked_add(length)? > limit {
            return None;
        }
        let from = out.len() - distance;
        // One byte at a time: a match can run on into what it is writing.
        for i in 0..length {
            let byte = out[from + i];
            out.push(byte);
        }
    }
    // Every squeezed form ends with bytes that had no match, even if there are none.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 1 << 24;

    fn round_trip(input: &[u8]) -> usize {
        let squeezed = squeeze(input);
        assert_eq!(expand(&squeezed, LIMIT).as_deref(), Some(input));
        squeezed.len()
    }

    #[test]
    fn it_comes_back_the_same_whatever_went_in() {
        assert_eq!(round_trip(b""), 1);
        round_trip(b"a");
        round_trip(b"abc");
        round_trip(b"abcd");
        round_trip(&[7; 100_000]);
        let mut x = 1u64;
        let noise: Vec<u8> = (0..70_000)
            .map(|_| {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                (x >> 56) as u8
            })
            .collect();
        round_trip(&noise);
        // A repeat further back than a distance can say, and lengths needing extra bytes.
        let mut far = noise.clone();
        far.extend_from_slice(&noise[..600]);
        round_trip(&far);
    }

    #[test]
    fn messages_that_look_alike_get_much_smaller() {
        let mut messages = Vec::new();
        for seq in 0u64..50 {
            messages.extend_from_slice(&1_793_820_572_028u64.wrapping_add(seq).to_le_bytes());
            messages.extend_from_slice(&[3, 0, 2, 1]);
            messages.extend_from_slice(&1_793_820_571_000u64.to_le_bytes());
        }
        let size = round_trip(&messages);
        assert!(size * 3 < messages.len(), "{size} of {}", messages.len());
    }

    #[test]
    fn nothing_sent_by_anyone_can_break_it() {
        assert_eq!(expand(&[], LIMIT), None);
        // A distance further back than anything written.
        assert_eq!(expand(&[0x10, b'a', 9, 0, 0x00], LIMIT), None);
        // A distance of nothing.
        assert_eq!(expand(&[0x10, b'a', 0, 0, 0x00], LIMIT), None);
        // Cut off in the bytes, in the distance and in a long length.
        assert_eq!(expand(&[0x50, b'a'], LIMIT), None);
        assert_eq!(expand(&[0x10, b'a', 1], LIMIT), None);
        assert_eq!(expand(&[0xF0, 255], LIMIT), None);
        // More than the limit, asked for by a short message.
        let bomb = squeeze(&[0; 100_000]);
        assert_eq!(expand(&bomb, 1000), None);
    }
}
