//! [`Score`], the number that points and the metrics built from them are counted in.

use std::{
    cmp::Ordering,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    iter::Sum,
    ops::{Add, AddAssign, Mul, Sub},
    str::FromStr,
};

use plugin_factory::schema::{DecimalRange, FieldKind, FieldType};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A number of points, or a tiebreak built from them: a finite `f64`, ordered totally so that it
/// can be compared, sorted and hashed.
///
/// It is never NaN or infinite, and `-0.0` is `0.0`. An operation whose result is not finite
/// panics.
///
/// # Examples
/// ```
/// use tournament_engine::Score;
///
/// const HALF: Score = Score::new(0.5);
///
/// assert_eq!(HALF + HALF, Score::ONE);
/// assert!(HALF < Score::ONE);
/// assert_eq!((HALF * Score::from(3u32)).to_string(), "1.5");
/// ```
#[derive(Default, Clone, Copy)]
pub struct Score(f64);

impl Score {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    /// The score `value` is.
    ///
    /// # Panics
    /// If `value` is NaN or infinite. In a constant, that fails the build.
    pub const fn new(value: f64) -> Self {
        match Self::finite(value) {
            Some(score) => score,
            None => panic!("a score is a finite number"),
        }
    }

    /// The score `value` is, if it is finite.
    const fn finite(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        // `-0.0` is `0.0`, so that equal scores are the same
        Some(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0.0
    }
}

// a score is never NaN, so `==` is reflexive, and never `-0.0`, so it agrees with `total_cmp`
impl PartialEq for Score {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Score {}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Score {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Hash for Score {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl From<u32> for Score {
    fn from(value: u32) -> Self {
        Self(value.into())
    }
}

/// The error of parsing a [`Score`] from a string that is not one.
#[derive(Debug, thiserror::Error)]
#[error("not a finite number: {0}")]
pub struct ParseScoreError(String);

/// Parses a number as an `f64` does, e.g. `-1.25`, rejecting NaN and infinity.
impl FromStr for Score {
    type Err = ParseScoreError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse()
            .ok()
            .and_then(Self::finite)
            .ok_or_else(|| ParseScoreError(text.to_owned()))
    }
}

/// Shows the score as its `f64` does, e.g. `3`, `1.5` and `-1.25`.
impl Display for Score {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

/// Shows the score as [`Display`] does.
impl Debug for Score {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

/// # Panics
/// If the sum is not finite.
impl Add for Score {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.0 + other.0)
    }
}

impl AddAssign for Score {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

/// # Panics
/// If the difference is not finite.
impl Sub for Score {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.0 - other.0)
    }
}

/// # Panics
/// If the product is not finite.
impl Mul for Score {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(self.0 * other.0)
    }
}

impl Sum for Score {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

/// A field of this type takes any number.
impl FieldType for Score {
    fn kind() -> FieldKind {
        FieldKind::Decimal(DecimalRange::ALL)
    }
}

impl Serialize for Score {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for Score {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        Self::finite(value)
            .ok_or_else(|| de::Error::custom(format!("a score is a finite number, not {value}")))
    }
}

#[cfg(test)]
mod tests {
    use std::hash::BuildHasher;

    use rustc_hash::FxBuildHasher;
    use serde_json::json;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn scores_are_ordered_by_value() {
        let mut scores = [2.0, -0.5, 1.5, 0.0].map(Score::new);

        scores.sort();

        assert_eq!(scores, [-0.5, 0.0, 1.5, 2.0].map(Score::new));
    }

    #[test]
    fn negative_zero_is_zero() {
        let negative_zero = Score::new(-0.0);

        assert_eq!(negative_zero, Score::ZERO);
        assert_eq!(negative_zero.cmp(&Score::ZERO), Ordering::Equal);
        assert_eq!(
            FxBuildHasher.hash_one(negative_zero),
            FxBuildHasher.hash_one(Score::ZERO)
        );
        assert_eq!((Score::new(-1.0) * Score::ZERO).to_string(), "0");
    }

    #[test]
    fn only_a_finite_number_is_a_score() {
        for text in ["NaN", "inf", "-inf", "one", ""] {
            assert!(text.parse::<Score>().is_err(), "{text:?} must be rejected");
        }
        assert_eq!("1.50".parse::<Score>().unwrap(), Score::new(1.5));
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", ignore = "wasm aborts on a panic")]
    #[should_panic(expected = "a score is a finite number")]
    fn an_operation_that_is_not_finite_panics() {
        let _ = Score::new(f64::MAX) + Score::new(f64::MAX);
    }

    #[test]
    fn displays_without_trailing_zeros() {
        let shown = [3.0, 1.5, 66.75, -1.25].map(|value| Score::new(value).to_string());

        assert_eq!(shown, ["3", "1.5", "66.75", "-1.25"]);
    }

    #[test]
    fn serializes_as_a_number() {
        assert_eq!(serde_json::to_value(Score::new(1.5)).unwrap(), json!(1.5));
        assert_eq!(
            serde_json::from_value::<Score>(json!(-2)).unwrap(),
            Score::new(-2.0)
        );
        let bytes = postcard::to_stdvec(&Score::new(0.1)).unwrap();
        assert_eq!(
            postcard::from_bytes::<Score>(&bytes).unwrap(),
            Score::new(0.1)
        );
    }
}
