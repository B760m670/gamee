//! Bitcoin-style compact difficulty encoding and retargeting — chosen
//! because it's a well-understood, well-precedented representation, not
//! because this chain needs to be bit-for-bit compatible with Bitcoin (it
//! doesn't: there's no sign bit reserved in the mantissa here, since this
//! format never needs to represent a negative target).
//!
//! `expand_target`/`compact_from_target` are exact — every bit of the
//! resulting 32-byte target is deterministic from the 4-byte compact form,
//! which matters because `Hash32::meets_target` is a bit-exact proof-of-
//! work check. `retarget`/`work_of` deliberately use `f64` internally: a
//! compact target only ever carries about 24 meaningful bits of precision
//! to begin with (the rest of its 256 bits are structural leading/trailing
//! zeros), so `f64`'s 52-bit mantissa loses nothing that matters, and it's
//! far less bug-prone than hand-rolled 256-bit multiply/divide for a
//! consensus-critical path that only needs correct *relative* ordering
//! (which chain has more work, how much easier/harder to retarget to), not
//! bit-exact big-integer accounting.

use crate::hash::Hash32;

pub type CompactTarget = u32;

/// Genesis-era target: mantissa `0xffffff` at exponent 30, i.e. about
/// 1/65536th of the maximum representable target — chosen so a block
/// needs on the order of 2^16 expected hash attempts (comfortably
/// sub-second even for unoptimized SHA-256, let alone a real phone CPU),
/// while still excluding all but a tiny fraction of the hash space, so an
/// actually-wrong proof-of-work still reliably fails this check (unlike
/// the maximum possible target, exponent 32, which very few hashes could
/// ever fail — not a meaningful test of anything). `expand_target`'s
/// `target = mantissa * 256^(exponent-3)` means a *larger* exponent (more
/// leading `0xff` bytes, closer to the 256-bit maximum) is *easier*, not
/// harder. The network starts trivially mineable by design and retargets
/// upward as real foreground+charging hashrate shows up over the first
/// ~24h (see `RETARGET_INTERVAL_BLOCKS`).
pub const INITIAL_DIFFICULTY_BITS: CompactTarget = 0x1eff_ffff;

pub const TARGET_BLOCK_TIME_SECS: i64 = 300; // 5 minutes
pub const RETARGET_INTERVAL_BLOCKS: u64 = 288; // ~24h at the target block time
const RETARGET_CLAMP_MIN: f64 = 0.25;
const RETARGET_CLAMP_MAX: f64 = 4.0;

/// Expands a compact `bits` value into the full 32-byte big-endian target
/// a block hash must not exceed. `bits` is laid out as `0xEEMMMMMM`: the
/// high byte `EE` is an exponent (in bytes, from the right), the low three
/// bytes `MMMMMM` are the mantissa. `target = mantissa * 256^(exponent-3)`
/// — for `exponent >= 3` that's the mantissa's three bytes placed starting
/// at index `32 - exponent` of the big-endian array (everything to the
/// right zero-padded, everything to the left implicitly zero); for
/// `exponent < 3` it's the mantissa right-shifted before landing in the
/// last few bytes.
pub fn expand_target(bits: CompactTarget) -> Hash32 {
    let exponent = (bits >> 24) as usize;
    let mantissa = bits & 0x00ff_ffff;
    let mut out = [0u8; 32];

    if mantissa == 0 {
        return Hash32(out);
    }

    let mantissa_bytes = [(mantissa >> 16) as u8, (mantissa >> 8) as u8, mantissa as u8];

    if exponent >= 3 {
        if exponent > 32 {
            // Degenerate: would shift the mantissa entirely out of the
            // 256-bit space. Never produced by `retarget` (clamped below),
            // only reachable from a hand-crafted `bits` value, in which
            // case treating it as "everything is zero, nothing satisfies
            // this target" is the safe behavior.
            return Hash32(out);
        }
        let start = 32 - exponent;
        for (i, byte) in mantissa_bytes.iter().enumerate() {
            let idx = start + i;
            if idx < 32 {
                out[idx] = *byte;
            }
        }
    } else {
        let shift_bits = 8 * (3 - exponent);
        let shifted = (mantissa as u64) >> shift_bits;
        out[24..32].copy_from_slice(&shifted.to_be_bytes());
    }

    Hash32(out)
}

/// The inverse of `expand_target`: finds the most-significant nonzero
/// byte, and takes it plus the next two bytes (zero-padded past the end
/// of the array) as the mantissa. Round-trips exactly for any target
/// actually produced by `expand_target`.
pub fn compact_from_target(target: &Hash32) -> CompactTarget {
    let bytes = target.0;
    let Some(first_nonzero) = bytes.iter().position(|&b| b != 0) else {
        return 0; // an all-zero target is never satisfiable by any real hash
    };
    let exponent = 32 - first_nonzero;
    let b0 = bytes[first_nonzero];
    let b1 = bytes.get(first_nonzero + 1).copied().unwrap_or(0);
    let b2 = bytes.get(first_nonzero + 2).copied().unwrap_or(0);
    let mantissa = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
    ((exponent as u32) << 24) | mantissa
}

/// Reads a target's magnitude as an `f64` — see the module doc for why an
/// approximation is acceptable here. Only used internally by `retarget`
/// and `work_of`, never for the exact proof-of-work check.
fn target_as_f64(target: &Hash32) -> f64 {
    let mut value = 0f64;
    for &byte in target.0.iter() {
        value = value * 256.0 + byte as f64;
    }
    value
}

fn f64_as_target(mut value: f64) -> Hash32 {
    if value < 0.0 {
        value = 0.0;
    }
    let max = target_as_f64(&Hash32([0xff; 32]));
    if value > max {
        value = max;
    }
    let mut bytes = [0u8; 32];
    for byte in bytes.iter_mut().rev() {
        *byte = (value % 256.0) as u8;
        value = (value / 256.0).floor();
    }
    Hash32(bytes)
}

/// Scales the target represented by `bits` by `actual_span_secs /
/// expected_span_secs`, clamped to `[0.25, 4.0]` so a single retarget
/// interval can't swing difficulty more than 4x in either direction (from
/// a burst of unusually fast/slow blocks, or clock skew), and re-encodes
/// the result. Blocks arriving slower than expected (`actual > expected`)
/// means mining was too hard, so the target *increases* (easier); faster
/// than expected means it *decreases* (harder).
pub fn retarget(bits: CompactTarget, actual_span_secs: i64, expected_span_secs: i64) -> CompactTarget {
    let ratio = (actual_span_secs as f64 / expected_span_secs as f64)
        .clamp(RETARGET_CLAMP_MIN, RETARGET_CLAMP_MAX);
    let target = expand_target(bits);
    let scaled = target_as_f64(&target) * ratio;
    compact_from_target(&f64_as_target(scaled))
}

/// The "work" one block at difficulty `bits` contributes to a chain's
/// cumulative total, for fork-choice comparison — inversely proportional
/// to the target, same shape as Bitcoin's `2^256/(target+1)`, just carried
/// in `f64` instead of a 256-bit integer division (see module doc).
/// Larger for harder (smaller-target) blocks.
pub fn work_of(bits: CompactTarget) -> f64 {
    let target = target_as_f64(&expand_target(bits));
    if target <= 0.0 {
        return f64::MAX;
    }
    let max_target = target_as_f64(&Hash32([0xff; 32]));
    max_target / (target + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_target_places_the_mantissa_at_the_expected_offset() {
        // exponent 3: mantissa occupies exactly the last 3 bytes.
        let target = expand_target(0x0300_ffff);
        assert_eq!(&target.0[29..32], &[0x00, 0xff, 0xff]);
        assert!(target.0[..29].iter().all(|&b| b == 0));
    }

    #[test]
    fn expand_target_shifts_left_for_larger_exponents() {
        let target = expand_target(0x0400_ffff); // one more byte of headroom
        assert_eq!(&target.0[28..31], &[0x00, 0xff, 0xff]);
        assert_eq!(target.0[31], 0x00);
    }

    #[test]
    fn expand_compact_round_trips_through_the_target_value() {
        // `compact_from_target` always produces the minimal/canonical
        // encoding of a target, which need not be byte-identical to an
        // arbitrary (possibly non-canonical) input `bits` — Bitcoin's own
        // genesis nBits (0x1d00ffff) has this same shape. What must hold
        // is that the *value* survives a round trip through the compact
        // form, since that's the only thing any comparison ever uses.
        for bits in [INITIAL_DIFFICULTY_BITS, 0x0400_ffff, 0x1d00_ffff, 0x0800_1234, 0x0300_0001] {
            let target = expand_target(bits);
            let recompacted = compact_from_target(&target);
            assert_eq!(
                expand_target(recompacted),
                target,
                "value did not survive round trip for {bits:#010x}"
            );
        }
    }

    #[test]
    fn compact_from_target_is_already_stable_for_a_canonical_encoding() {
        // A canonical `bits` value (mantissa's own top byte nonzero) does
        // round-trip byte-for-byte, since there's no more-minimal encoding
        // of the same value to normalize toward.
        let canonical = 0x1cff_ff00; // exponent 28, mantissa 0xffff00
        let target = expand_target(canonical);
        assert_eq!(compact_from_target(&target), canonical);
    }

    #[test]
    fn zero_mantissa_expands_to_an_unsatisfiable_all_zero_target() {
        let target = expand_target(0x0400_0000);
        assert_eq!(target, Hash32::ZERO);
    }

    /// A deliberately mid-range target (not `INITIAL_DIFFICULTY_BITS`, and
    /// not near either end of the encodable range) for the retarget
    /// directionality/clamping tests below, so scaling it up or down by up
    /// to 4x never runs into the format's own ceiling or floor — which
    /// would otherwise mask the very thing these tests check.
    const SAMPLE_MID_RANGE_BITS: CompactTarget = 0x10ff_ffff;

    #[test]
    fn retarget_increases_the_target_when_blocks_are_slower_than_expected() {
        let bits = SAMPLE_MID_RANGE_BITS;
        let retargeted = retarget(bits, TARGET_BLOCK_TIME_SECS * 2, TARGET_BLOCK_TIME_SECS);
        assert!(
            target_as_f64(&expand_target(retargeted)) > target_as_f64(&expand_target(bits)),
            "slower-than-expected blocks should make the target larger (easier)"
        );
    }

    #[test]
    fn retarget_decreases_the_target_when_blocks_are_faster_than_expected() {
        let bits = SAMPLE_MID_RANGE_BITS;
        let retargeted = retarget(bits, TARGET_BLOCK_TIME_SECS / 2, TARGET_BLOCK_TIME_SECS);
        assert!(
            target_as_f64(&expand_target(retargeted)) < target_as_f64(&expand_target(bits)),
            "faster-than-expected blocks should make the target smaller (harder)"
        );
    }

    #[test]
    fn retarget_clamps_extreme_ratios_to_4x() {
        let bits = SAMPLE_MID_RANGE_BITS;
        let unclamped_slower = retarget(bits, TARGET_BLOCK_TIME_SECS * 100, TARGET_BLOCK_TIME_SECS);
        let clamped_at_4x = retarget(bits, TARGET_BLOCK_TIME_SECS * 4, TARGET_BLOCK_TIME_SECS);
        let ratio = target_as_f64(&expand_target(unclamped_slower)) / target_as_f64(&expand_target(bits));
        assert!((ratio - 4.0).abs() < 0.01, "ratio should be clamped to ~4x, was {ratio}");
        assert_eq!(unclamped_slower, clamped_at_4x);
    }

    #[test]
    fn smaller_target_has_more_work() {
        let easy = work_of(INITIAL_DIFFICULTY_BITS);
        let harder_bits = retarget(INITIAL_DIFFICULTY_BITS, TARGET_BLOCK_TIME_SECS / 4, TARGET_BLOCK_TIME_SECS);
        let hard = work_of(harder_bits);
        assert!(hard > easy, "a smaller target must represent more work");
    }

    #[test]
    fn work_is_always_positive_and_finite() {
        assert!(work_of(INITIAL_DIFFICULTY_BITS).is_finite());
        assert!(work_of(INITIAL_DIFFICULTY_BITS) > 0.0);
    }
}
