//! Utilities for common bitwise ops.

use num_traits::{PrimInt, Unsigned};

/// Twiddles bits from a type `ty` into a new value of type `ty`
///
/// ## Safety
/// - The ranges are not checked for validity.
/// - The resulting value is not checked to overflow the type.
#[macro_export]
macro_rules! twiddle {
    ($ty:ty, $value:expr, $($range:expr),+ $(,)?) => {{
        let mut result: $ty = 0;

        $(
            let bits = $crate::bits!($ty, $value, $range);
            let width = $range.end - $range.start;
            result = (result << width) | bits;
        )+

        result
    }};
}

/// Extracts a range of bits from a value.
///
/// ## Safety
/// - The range is not checked for validity.
#[macro_export]
macro_rules! bits {
    ($ty:ty, $value:expr, $range:expr) => {{
        let (start, width) =
            (|range: core::ops::Range<usize>| (range.start, range.end - range.start))($range);
        (($value >> start) & ((1 << width) - 1)) as $ty
    }};
}

/// Perform a sign extension of a value embedded in the lower bits of `data` up to
/// the `index`th bit.
#[inline(always)]
pub fn sign_extend<T>(data: T, index: T) -> T
where
    T: PrimInt + Unsigned,
{
    let index = index.to_usize().expect("index is too large for usize");
    let field_size = T::zero().count_zeros() as usize;

    debug_assert!(index < field_size, "index is too large for sign-extension over the field size");

    let is_signed = data & (T::one() << index);
    if is_signed == T::zero() {
        data & (T::max_value() >> (field_size - 1 - index))
    } else {
        data | ((T::max_value() >> index) << index)
    }
}

#[cfg(test)]
mod test {
    use rstest::rstest;

    use crate::XWord;

    #[test]
    fn test_bits_simple() {
        let value = 0b1010;
        let result = bits!(u64, value, 0..4);
        assert_eq!(result, value);

        let value = 0b1010;
        let result = bits!(u64, value, 1..3);
        assert_eq!(result, 1);
    }

    #[test]
    fn test_twiddle_simple() {
        let value = 0b1010;
        let result = twiddle!(u64, value, 0..4);
        assert_eq!(result, value);

        let value = 0b1100;
        let result = twiddle!(u64, value, 3..4, 1..2, 2..3, 0..1);
        assert_eq!(result, 0b1010);
    }

    #[rstest]
    #[case(0b1111, 3)]
    #[case(0b1010, 3)]
    #[case(0b1010, 2)]
    #[case(0b1010, 1)]
    #[case(0b1010, 0)]
    #[case(XWord::MAX, (XWord::BITS - 1) as XWord)]
    #[case(0, (XWord::BITS - 1) as XWord)]
    #[should_panic]
    #[case(XWord::MAX, 64)]
    fn test_sign_extension_u64_simple(#[case] data: XWord, #[case] index: XWord) {
        if (data >> index) & 1 == 1 {
            let ones = XWord::MAX << index;
            let mask = !ones;
            assert_eq!(super::sign_extend(data, index), (data & mask) | ones);
        } else {
            let mask = !(XWord::MAX << index);
            assert_eq!(super::sign_extend(data, index), data & mask);
        }
    }
}
