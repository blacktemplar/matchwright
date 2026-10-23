//! The numbers a numeric field takes: [`IntegerRange`] and [`DecimalRange`].

use std::{
    fmt::{self, Display, Formatter},
    num::NonZeroU64,
};

/// The whole numbers an [`Integer`](super::FieldKind::Integer) field takes: those from `min` to
/// `max` that are multiples of `multiple_of`.
///
/// `min` and `max` are always multiples of `multiple_of` themselves, so a UI can step from `min`
/// by `multiple_of`.
///
/// A bound is either stated, i.e. chosen for what the number means, or only the limit of the type
/// holding it, such as `u32::MAX`. Both bound the range, but only a stated one is meant for an
/// operator to read, see [`stated_min`](Self::stated_min) and [`Display`].
///
/// # Examples
/// ```
/// use plugin_factory::schema::IntegerRange;
///
/// let rounds = IntegerRange::limits(0, u32::MAX.into()).at_least(1);
///
/// assert!(rounds.contains(10));
/// assert!(!rounds.contains(0));
/// assert_eq!(rounds.max(), 4294967295);
/// assert_eq!(rounds.stated_max(), None);
/// assert_eq!(rounds.to_string(), "a whole number of at least 1");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerRange {
    min: i64,
    max: i64,
    multiple_of: NonZeroU64,
    min_stated: bool,
    max_stated: bool,
}

impl IntegerRange {
    /// Every whole number an `i64` holds.
    pub const ALL: Self = Self::limits(i64::MIN, i64::MAX);

    /// Every whole number from `min` to `max`, both stated.
    ///
    /// # Panics
    /// If `min` is greater than `max`.
    pub const fn new(min: i64, max: i64) -> Self {
        Self {
            min_stated: true,
            max_stated: true,
            ..Self::limits(min, max)
        }
    }

    /// Every whole number from `min` to `max`, the limits of a type rather than stated bounds.
    ///
    /// # Panics
    /// If `min` is greater than `max`.
    pub const fn limits(min: i64, max: i64) -> Self {
        assert!(
            min <= max,
            "the minimum of a range is greater than its maximum"
        );
        Self {
            min,
            max,
            multiple_of: NonZeroU64::MIN,
            min_stated: false,
            max_stated: false,
        }
    }

    /// The least number of the range, stated or not.
    pub const fn min(&self) -> i64 {
        self.min
    }

    /// The greatest number of the range, stated or not.
    pub const fn max(&self) -> i64 {
        self.max
    }

    /// The least number of the range if it is stated, [`None`] if it is only a type's limit.
    pub const fn stated_min(&self) -> Option<i64> {
        if self.min_stated {
            Some(self.min)
        } else {
            None
        }
    }

    /// The greatest number of the range if it is stated, [`None`] if it is only a type's limit.
    pub const fn stated_max(&self) -> Option<i64> {
        if self.max_stated {
            Some(self.max)
        } else {
            None
        }
    }

    /// This range narrowed to the numbers from `min` on, stated.
    ///
    /// # Panics
    /// If `min` is not a multiple of what the range takes multiples of, or greater than its
    /// maximum.
    #[must_use]
    pub fn at_least(self, min: i64) -> Self {
        self.narrowed_to_at_least(min as f64)
            .unwrap_or_else(|reason| panic!("the range cannot be narrowed: {reason}"))
    }

    /// This range narrowed to the numbers up to `max`, stated.
    ///
    /// # Panics
    /// If `max` is not a multiple of what the range takes multiples of, or less than its minimum.
    #[must_use]
    pub fn at_most(self, max: i64) -> Self {
        self.narrowed_to_at_most(max as f64)
            .unwrap_or_else(|reason| panic!("the range cannot be narrowed: {reason}"))
    }

    /// What every number of the range is a multiple of, 1 unless narrowed.
    pub const fn multiple_of(&self) -> u64 {
        self.multiple_of.get()
    }

    /// This range narrowed to the multiples of `multiple_of`, with its bounds moved inward to the
    /// nearest of them.
    ///
    /// # Panics
    /// If `multiple_of` is 0 or not a multiple of what the range takes multiples of already, or no
    /// number of the range is a multiple of it.
    #[must_use]
    pub fn multiples_of(self, multiple_of: u64) -> Self {
        self.narrowed_to_multiples_of(multiple_of as f64)
            .unwrap_or_else(|reason| panic!("the range cannot be narrowed: {reason}"))
    }

    /// Whether `value` is one of the numbers of the range.
    pub const fn contains(&self, value: i64) -> bool {
        self.min <= value
            && value <= self.max
            && value.unsigned_abs().is_multiple_of(self.multiple_of())
    }

    /// This range narrowed to the multiples of `multiple_of`, with its bounds moved inward to the
    /// nearest of them.
    pub(super) fn narrowed_to_multiples_of(self, multiple_of: f64) -> Result<Self, String> {
        let multiple_of = whole(multiple_of)
            .and_then(|multiple_of| u64::try_from(multiple_of).ok())
            .and_then(NonZeroU64::new)
            .ok_or_else(|| format!("{multiple_of} is no positive whole number"))?;
        if !multiple_of.get().is_multiple_of(self.multiple_of()) {
            return Err(format!(
                "{multiple_of} is not a multiple of {}",
                self.multiple_of()
            ));
        }
        let step = i128::from(multiple_of.get());
        let min = i128::from(self.min).div_euclid(step) * step;
        let min = if min < i128::from(self.min) {
            min + step
        } else {
            min
        };
        let max = i128::from(self.max).div_euclid(step) * step;
        match (i64::try_from(min), i64::try_from(max)) {
            (Ok(min), Ok(max)) if min <= max => Ok(Self {
                min,
                max,
                multiple_of,
                ..self
            }),
            _ => Err(format!("no multiple of {multiple_of} is {self}")),
        }
    }

    /// This range narrowed to the numbers from `min` on, with its minimum stated.
    pub(super) fn narrowed_to_at_least(self, min: f64) -> Result<Self, String> {
        let min = self.bound(min)?;
        Self {
            min_stated: true,
            ..self
        }
        .narrowed(min.max(self.min), self.max)
    }

    /// This range narrowed to the numbers up to `max`, with its maximum stated.
    pub(super) fn narrowed_to_at_most(self, max: f64) -> Result<Self, String> {
        let max = self.bound(max)?;
        Self {
            max_stated: true,
            ..self
        }
        .narrowed(self.min, max.min(self.max))
    }

    /// `bound` as a bound of this range: a whole number and a multiple of its `multiple_of`.
    fn bound(&self, bound: f64) -> Result<i64, String> {
        whole(bound)
            .filter(|bound| bound.unsigned_abs().is_multiple_of(self.multiple_of()))
            .ok_or_else(|| {
                format!(
                    "{bound} is no whole number that is a multiple of {}",
                    self.multiple_of()
                )
            })
    }

    fn narrowed(self, min: i64, max: i64) -> Result<Self, String> {
        if min > max {
            return Err(format!("no number from {min} to {max} is {self}"));
        }
        Ok(Self { min, max, ..self })
    }
}

/// `value` as an `i64`, if it is a whole number one holds.
fn whole(value: f64) -> Option<i64> {
    // `as` saturates, so converting back gives the same float only for a whole number in range
    (value as i64 as f64 == value && value < i64::MAX as f64).then_some(value as i64)
}

/// Describes the numbers for an operator, e.g. "a whole number from 1 to 10" or "a whole number
/// that is a multiple of 2", leaving out a bound that is only a type's limit.
impl Display for IntegerRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("a whole number")?;
        match (self.stated_min(), self.stated_max()) {
            (Some(min), Some(max)) => write!(f, " from {min} to {max}")?,
            (Some(min), None) => write!(f, " of at least {min}")?,
            (None, Some(max)) => write!(f, " of at most {max}")?,
            (None, None) => {}
        }
        if self.multiple_of() != 1 {
            write!(f, " that is a multiple of {}", self.multiple_of)?;
        }
        Ok(())
    }
}

/// The numbers a [`Decimal`](super::FieldKind::Decimal) field takes: those from `min` to `max`
/// that are multiples of `multiple_of`, each only where given.
///
/// `multiple_of` is a positive whole number or a power of two, such as `0.5` or `0.25`, so that
/// whether a number is a multiple of it is decided exactly. `min` and `max` are multiples of it
/// themselves, so a UI can step from `min` by `multiple_of`.
///
/// # Examples
/// ```
/// use plugin_factory::schema::DecimalRange;
///
/// let quarters = DecimalRange::ALL.multiples_of(0.25);
///
/// assert!(quarters.contains(-1.75));
/// assert!(!quarters.contains(0.1));
/// assert_eq!(quarters.to_string(), "a number that is a multiple of 0.25");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecimalRange {
    min: Option<f64>,
    max: Option<f64>,
    multiple_of: Option<f64>,
}

// every number of a range is finite, so no `NaN` breaks the reflexivity of `==`
impl Eq for DecimalRange {}

impl DecimalRange {
    /// Every finite number.
    pub const ALL: Self = Self {
        min: None,
        max: None,
        multiple_of: None,
    };

    /// This range narrowed to the multiples of `multiple_of`.
    ///
    /// # Panics
    /// If `multiple_of` is neither a positive whole number nor a power of two, if this range takes
    /// multiples of another number already, or if it has bounds.
    #[must_use]
    pub const fn multiples_of(self, multiple_of: f64) -> Self {
        assert!(
            is_exact_multiple_of(multiple_of),
            "a range takes the multiples of a positive whole number or of a power of two only"
        );
        assert!(
            self.multiple_of.is_none() && self.min.is_none() && self.max.is_none(),
            "a range is narrowed to multiples before it gets bounds"
        );
        Self {
            multiple_of: Some(multiple_of),
            ..self
        }
    }

    /// This range narrowed to the numbers from `min` on.
    ///
    /// # Panics
    /// If `min` is not finite, not a multiple of what the range takes multiples of, or greater
    /// than its maximum.
    #[must_use]
    pub fn at_least(self, min: f64) -> Self {
        self.narrowed_to_at_least(min)
            .unwrap_or_else(|reason| panic!("the range cannot be narrowed: {reason}"))
    }

    /// This range narrowed to the numbers up to `max`.
    ///
    /// # Panics
    /// If `max` is not finite, not a multiple of what the range takes multiples of, or less than
    /// its minimum.
    #[must_use]
    pub fn at_most(self, max: f64) -> Self {
        self.narrowed_to_at_most(max)
            .unwrap_or_else(|reason| panic!("the range cannot be narrowed: {reason}"))
    }

    pub const fn min(&self) -> Option<f64> {
        self.min
    }

    pub const fn max(&self) -> Option<f64> {
        self.max
    }

    pub const fn multiple_of(&self) -> Option<f64> {
        self.multiple_of
    }

    /// Whether `value` is one of the numbers of the range.
    pub fn contains(&self, value: f64) -> bool {
        // `%` on floats is exact
        value.is_finite()
            && self.min.is_none_or(|min| min <= value)
            && self.max.is_none_or(|max| value <= max)
            && self
                .multiple_of
                .is_none_or(|multiple_of| value % multiple_of == 0.0)
    }

    /// This range narrowed to the multiples of `multiple_of`, with its bounds moved inward to the
    /// nearest of them.
    pub(super) fn narrowed_to_multiples_of(self, multiple_of: f64) -> Result<Self, String> {
        if !is_exact_multiple_of(multiple_of) {
            return Err(format!(
                "{multiple_of} is neither a positive whole number nor a power of two"
            ));
        }
        if let Some(old) = self.multiple_of
            && multiple_of % old != 0.0
        {
            return Err(format!("{multiple_of} is not a multiple of {old}"));
        }
        let min = self.min.map(|min| (min / multiple_of).ceil() * multiple_of);
        let max = self
            .max
            .map(|max| (max / multiple_of).floor() * multiple_of);
        Self {
            multiple_of: Some(multiple_of),
            ..self
        }
        .narrowed(min, max)
    }

    /// This range narrowed to the numbers from `min` on.
    pub(super) fn narrowed_to_at_least(self, min: f64) -> Result<Self, String> {
        let min = self.bound(min)?;
        self.narrowed(Some(self.min.map_or(min, |old| old.max(min))), self.max)
    }

    /// This range narrowed to the numbers up to `max`.
    pub(super) fn narrowed_to_at_most(self, max: f64) -> Result<Self, String> {
        let max = self.bound(max)?;
        self.narrowed(self.min, Some(self.max.map_or(max, |old| old.min(max))))
    }

    /// `bound` as a bound of this range: a finite number and a multiple of its `multiple_of`.
    fn bound(&self, bound: f64) -> Result<f64, String> {
        if !bound.is_finite() {
            return Err(format!("{bound} is no finite number"));
        }
        match self.multiple_of {
            Some(multiple_of) if bound % multiple_of != 0.0 => {
                Err(format!("{bound} is not a multiple of {multiple_of}"))
            }
            _ => Ok(bound),
        }
    }

    fn narrowed(self, min: Option<f64>, max: Option<f64>) -> Result<Self, String> {
        if let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            return Err(format!("no number from {min} to {max} is {self}"));
        }
        Ok(Self { min, max, ..self })
    }
}

/// Whether `multiple_of` is a positive whole number or a power of two, whose multiples a float
/// can be checked to be exactly.
const fn is_exact_multiple_of(multiple_of: f64) -> bool {
    const MANTISSA: u64 = (1 << 52) - 1;
    multiple_of.is_finite()
        && multiple_of > 0.0
        && (multiple_of as u64 as f64 == multiple_of
            || multiple_of.is_normal() && multiple_of.to_bits() & MANTISSA == 0)
}

/// Describes the numbers, e.g. "a number of at least 0 that is a multiple of 0.25".
impl Display for DecimalRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("a number")?;
        match (self.min, self.max) {
            (Some(min), Some(max)) => write!(f, " from {min} to {max}")?,
            (Some(min), None) => write!(f, " of at least {min}")?,
            (None, Some(max)) => write!(f, " of at most {max}")?,
            (None, None) => {}
        }
        if let Some(multiple_of) = self.multiple_of {
            write!(f, " that is a multiple of {multiple_of}")?;
        }
        Ok(())
    }
}

/// The numbers a numeric field takes, as an [`OutOfRange`](crate::config::ConfigError::OutOfRange)
/// error names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberRange {
    Integer(IntegerRange),
    Decimal(DecimalRange),
}

impl Display for NumberRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(range) => Display::fmt(range, f),
            Self::Decimal(range) => Display::fmt(range, f),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn an_integer_range_takes_the_multiples_between_its_bounds() {
        let even = IntegerRange::new(-10, 10)
            .narrowed_to_multiples_of(2.0)
            .unwrap();

        assert!(even.contains(-10) && even.contains(0) && even.contains(10));
        assert!(!even.contains(3) && !even.contains(12) && !even.contains(-12));
    }

    #[test]
    fn narrowing_to_multiples_moves_the_bounds_inward() {
        let range = IntegerRange::new(-7, 7)
            .narrowed_to_multiples_of(3.0)
            .unwrap();
        assert_eq!((range.min(), range.max()), (-6, 6));

        let range = DecimalRange::ALL
            .narrowed_to_at_least(-1.0)
            .and_then(|range| range.narrowed_to_at_most(1.0))
            .and_then(|range| range.narrowed_to_multiples_of(4.0))
            .unwrap();
        assert_eq!((range.min(), range.max()), (Some(0.0), Some(0.0)));
    }

    #[test]
    fn a_bound_has_to_be_a_multiple() {
        let even = IntegerRange::ALL.narrowed_to_multiples_of(2.0).unwrap();
        assert!(even.narrowed_to_at_least(1.0).is_err());
        assert_eq!(even.narrowed_to_at_least(2.0).unwrap().min(), 2);

        let quarters = DecimalRange::ALL.multiples_of(0.25);
        assert!(quarters.narrowed_to_at_least(0.1).is_err());
        assert_eq!(quarters.narrowed_to_at_least(0.5).unwrap().min(), Some(0.5));
    }

    /// A type's limit still bounds the range, but only a bound narrowing it is stated.
    #[test]
    fn a_narrowed_bound_is_stated_and_a_limit_is_not() {
        let rounds = IntegerRange::limits(0, 255)
            .narrowed_to_at_least(1.0)
            .unwrap();

        assert_eq!((rounds.min(), rounds.max()), (1, 255));
        assert_eq!((rounds.stated_min(), rounds.stated_max()), (Some(1), None));
        assert!(!rounds.contains(256));
        let even = rounds.narrowed_to_multiples_of(2.0).unwrap();
        assert_eq!((even.stated_min(), even.stated_max()), (Some(2), None));
        assert_eq!(even.max(), 254);
    }

    #[test]
    fn a_narrowed_bound_never_widens_the_range() {
        let positive = IntegerRange::new(1, 100);

        assert_eq!(positive.narrowed_to_at_least(0.0).unwrap().min(), 1);
        assert_eq!(positive.narrowed_to_at_most(1000.0).unwrap().max(), 100);
        assert!(positive.narrowed_to_at_most(0.0).is_err());
    }

    #[test]
    fn narrowed_multiples_are_multiples_of_the_ones_before() {
        let quarters = DecimalRange::ALL.multiples_of(0.25);

        assert!(quarters.narrowed_to_multiples_of(0.5).is_ok());
        assert!(quarters.narrowed_to_multiples_of(0.125).is_err());
        assert!(IntegerRange::ALL.narrowed_to_multiples_of(0.5).is_err());
    }

    #[test]
    fn only_whole_numbers_and_powers_of_two_are_multiples_checked_exactly() {
        for multiple_of in [1.0, 3.0, 0.5, 0.25, 1.0 / 1024.0] {
            assert!(is_exact_multiple_of(multiple_of), "{multiple_of}");
        }
        for multiple_of in [0.1, 0.75, 0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(!is_exact_multiple_of(multiple_of), "{multiple_of}");
        }
    }

    #[test]
    fn a_decimal_range_checks_multiples_exactly() {
        let quarters = DecimalRange::ALL.multiples_of(0.25);

        assert!(quarters.contains(66.75) && quarters.contains(-0.25));
        assert!(!quarters.contains(0.1) && !quarters.contains(f64::NAN));
        assert!(DecimalRange::ALL.contains(0.1));
    }

    #[test]
    fn a_range_describes_only_what_narrows_it() {
        assert_eq!(IntegerRange::ALL.to_string(), "a whole number");
        assert_eq!(IntegerRange::limits(0, 255).to_string(), "a whole number");
        assert_eq!(
            IntegerRange::limits(0, 255).at_least(1).to_string(),
            "a whole number of at least 1"
        );
        assert_eq!(
            IntegerRange::new(1, 10).to_string(),
            "a whole number from 1 to 10"
        );
        assert_eq!(
            DecimalRange::ALL
                .multiples_of(0.5)
                .narrowed_to_at_least(0.0)
                .unwrap()
                .to_string(),
            "a number of at least 0 that is a multiple of 0.5"
        );
    }
}
