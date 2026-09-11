//! Policies for encoding and decoding floating-point values.

use crate::error::{invalid_value, ReadResult, WriteError, WriteResult};

/// Controls floating-point validation for a [`crate::config::Configuration`].
pub trait FloatEncoding: 'static {
    /// Whether floats may be deserialized through the borrowed zero-copy path.
    const ZERO_COPY: bool;

    fn validate_f32_write(value: f32) -> WriteResult<()>;
    fn validate_f64_write(value: f64) -> WriteResult<()>;
    fn validate_f32_read(value: f32) -> ReadResult<()>;
    fn validate_f64_read(value: f64) -> ReadResult<()>;
}

/// Marker for float policies that permit borrowed zero-copy decoding.
pub trait ZeroCopyFloatEncoding {}

/// Preserves Wincode's historical behavior, including all IEEE-754 bit patterns.
pub struct AllowNaN;

/// Rejects NaN values while preserving finite values, infinities, and signed zero.
pub struct RejectNaN;

impl FloatEncoding for AllowNaN {
    const ZERO_COPY: bool = true;

    #[inline(always)]
    fn validate_f32_write(_value: f32) -> WriteResult<()> {
        Ok(())
    }
    #[inline(always)]
    fn validate_f64_write(_value: f64) -> WriteResult<()> {
        Ok(())
    }
    #[inline(always)]
    fn validate_f32_read(_value: f32) -> ReadResult<()> {
        Ok(())
    }
    #[inline(always)]
    fn validate_f64_read(_value: f64) -> ReadResult<()> {
        Ok(())
    }
}

impl ZeroCopyFloatEncoding for AllowNaN {}

impl FloatEncoding for RejectNaN {
    const ZERO_COPY: bool = false;

    #[inline(always)]
    fn validate_f32_write(value: f32) -> WriteResult<()> {
        if value.is_nan() {
            Err(WriteError::Custom(
                "NaN is not supported by this float encoding",
            ))
        } else {
            Ok(())
        }
    }
    #[inline(always)]
    fn validate_f64_write(value: f64) -> WriteResult<()> {
        if value.is_nan() {
            Err(WriteError::Custom(
                "NaN is not supported by this float encoding",
            ))
        } else {
            Ok(())
        }
    }
    #[inline(always)]
    fn validate_f32_read(value: f32) -> ReadResult<()> {
        if value.is_nan() {
            Err(invalid_value("NaN is not supported by this float encoding"))
        } else {
            Ok(())
        }
    }
    #[inline(always)]
    fn validate_f64_read(value: f64) -> ReadResult<()> {
        if value.is_nan() {
            Err(invalid_value("NaN is not supported by this float encoding"))
        } else {
            Ok(())
        }
    }
}
