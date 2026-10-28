use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Mul, Sub};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 直接效率的唯一运行时表示。内部保存千分倍率；`1000` 表示 `1.000`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Efficiency(i32);

impl Efficiency {
    pub const SCALE: i32 = 1000;
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(Self::SCALE);

    pub const fn from_millis(raw: i32) -> Self {
        Self(raw)
    }

    pub fn from_decimal(value: f64) -> Self {
        assert!(value.is_finite(), "efficiency must be finite");
        let scaled = (value * f64::from(Self::SCALE)).round();
        assert!(
            scaled >= f64::from(i32::MIN) && scaled <= f64::from(i32::MAX),
            "efficiency out of range: {value}"
        );
        Self(scaled as i32)
    }

    pub(crate) fn from_percent_points(value: f64) -> Self {
        Self::from_decimal(value / 100.0)
    }

    pub const fn millis(self) -> i32 {
        self.0
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.0) / f64::from(Self::SCALE)
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// 两个千分效率相乘，并在结果处四舍五入到三位小数。
    pub fn multiply(self, rhs: Self) -> Self {
        let product = i64::from(self.0) * i64::from(rhs.0);
        let scale = i64::from(Self::SCALE);
        let rounded = if product >= 0 {
            (product + scale / 2) / scale
        } else {
            (product - scale / 2) / scale
        };
        Self(i32::try_from(rounded).expect("efficiency multiplication overflow"))
    }

    pub fn scale_ratio(self, numerator: i64, denominator: i64) -> Self {
        assert!(
            denominator > 0,
            "efficiency ratio denominator must be positive"
        );
        let product = i64::from(self.0) * numerator;
        let rounded = if product >= 0 {
            (product + denominator / 2) / denominator
        } else {
            (product - denominator / 2) / denominator
        };
        Self(i32::try_from(rounded).expect("efficiency ratio overflow"))
    }
}

impl Add for Efficiency {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_add(rhs.0)
                .expect("efficiency addition overflow"),
        )
    }
}

impl AddAssign for Efficiency {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Efficiency {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs.0)
                .expect("efficiency subtraction overflow"),
        )
    }
}

impl Mul for Efficiency {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.multiply(rhs)
    }
}

impl Sum for Efficiency {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |total, value| total + value)
    }
}

impl fmt::Display for Efficiency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.as_f64())
    }
}

impl Serialize for Efficiency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.as_f64())
    }
}

impl<'de> Deserialize<'de> for Efficiency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        if !value.is_finite() {
            return Err(serde::de::Error::custom("efficiency must be finite"));
        }
        let scaled = (value * f64::from(Self::SCALE)).round();
        if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
            return Err(serde::de::Error::custom("efficiency out of range"));
        }
        Ok(Self::from_decimal(value))
    }
}
