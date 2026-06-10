//! # qoick
//!
//! faster than ever, even than the `image` or `qoi` crates!

#![no_std]
extern crate alloc;

mod encode;
mod prepare;
use alloc::vec::Vec;

#[cfg(feature = "image")]
use image::DynamicImage;

use crate::encode::Buffer;
use crate::encode::qoi_encode_generic;
use crate::prepare::QOI_HEADER_SIZE;
use crate::prepare::QOI_PADDING_SIZE;
use crate::prepare::qoi_reserve_and_encode;

#[derive(Debug)]
pub enum QoiEncodeError {
    /// Provided image file can not possibly fit in the address space of RAM. It
    /// usually means your image dimensions are malformed, or the image is
    /// bigger than the images representable by your address space.
    SizeOverflow,

    /// Given byte slice did not have the exact amount of required pixel data.
    InvalidInput,
}

/// Encodes a dynamic image from the `image` crate. If the underlying surface
/// is not RGB or RGBA, it makes a temporary copy.  
#[cfg(feature = "image")]
#[inline] // this is just a match, it should be inlined.
pub fn qoi_encode(
    img: &image::DynamicImage, out: &mut Vec<u8>,
) -> Result<usize, QoiEncodeError> {
    match img {
        DynamicImage::ImageRgb8(rgb) => qoi_encode_rgb_image(rgb, out),
        DynamicImage::ImageRgba8(rgba) => qoi_encode_rgba_image(rgba, out),
        _ =>
            if img.has_alpha() {
                qoi_encode_rgba_image(&img.to_rgba8(), out)
            } else {
                qoi_encode_rgb_image(&img.to_rgb8(), out)
            },
    }
}

/// Biggest possible representation of a QOI image is comprised of all
/// QOI_OP_RGBA or QOI_OP_RGB pixels, depending on the colorspace. Each
/// QOI_OP_RGBA takes 5 bytes (1 tag byte, 4 raw subpixel bytes), and each
/// QOI_OP_RGB similarly takes 4 bytes to encode.
///
/// This translates to the number `num_pixels + num_pixels * channels`
/// bytes.
#[inline]
pub fn qoi_encode_buf_size(channels: u8, w: u32, h: u32) -> Option<u64> {
    assert!(
        channels == 3 || channels == 4,
        "QOI format only supports 3 or 4 channels"
    );

    let num_pixels = w as u64 * h as u64; // can not overflow since u64

    Some(
        QOI_HEADER_SIZE as u64
            // each pixel must at least store 1 tag byte
            + num_pixels
            // each pixel stores at least `channels` bytes of data
            // this empirically never errors (images of size 2 billion by
            // 2 billion are exceptionally rare other than malformed images) 
            + num_pixels.checked_mul(channels as u64)?
            + QOI_PADDING_SIZE as u64,
    )
}

/// Encode an RGBA image provided by the `image_ptr` into the `out_ptr`, where
/// `out_ptr` has enough capacity to hold any possible QOI image. You can get
/// this number by calling [`qoi_encode_buf_size`].
///
/// Returns the amount of bytes written to `out_ptr`.
///
/// # Safety
///
/// - `width as usize * height as usize` must not be bigger than `isize::MAX`,
///   otherwise the pointer wraps around the address space on 32 bit targets.
/// - `image_ptr` must be valid for reads of `width * height * 4` byte size.
/// - `out_ptr` must be valid for writes of `qoi_encode_buf_size(4, w, h)` byte
///   size.
#[inline(never)]
pub unsafe fn qoi_raw_encode_4_channels(
    image_ptr: *const u8, width: u32, height: u32, out_ptr: *mut u8,
) -> usize {
    unsafe {
        qoi_encode_generic::<4>(image_ptr, width, height, Buffer {
            ptr: out_ptr,
        })
        .ptr
        .addr()
        .unchecked_sub(out_ptr.addr())
    }
}

/// Encode an RGB image provided by the `image_ptr` into the `out_ptr`, where
/// `out_ptr` has enough capacity to hold any possible QOI image. You can get
/// this number by calling [`qoi_encode_buf_size`].
///
/// Returns the amount of bytes written to `out_ptr`.
///
/// # Safety
///
/// - `width as usize * height as usize` must not be bigger than `isize::MAX`,
///   otherwise the pointer wraps around the address space on 32 bit targets.
/// - `image_ptr` must be valid for reads of `width * height * 3` byte size.
/// - `out_ptr` must be valid for writes of `qoi_encode_buf_size(3, w, h)` byte
///   size.
#[inline(never)]
pub unsafe fn qoi_raw_encode_3_channels(
    image_ptr: *const u8, width: u32, height: u32, out_ptr: *mut u8,
) -> usize {
    unsafe {
        qoi_encode_generic::<3>(image_ptr, width, height, Buffer {
            ptr: out_ptr,
        })
        .ptr
        .addr()
        .unchecked_sub(out_ptr.addr())
    }
}

/// Encode an RGB image provided by the `bytes` into the `out` Vec.
///
/// `bytes` must hold exactly `width * height * 3` bytes.
#[inline]
pub fn qoi_encode_rgb_bytes(
    bytes: &[u8], width: u32, height: u32, out: &mut Vec<u8>,
) -> Result<(), QoiEncodeError> {
    let expected_size = usize::try_from(width as u64 * height as u64)
        .map_err(|_| QoiEncodeError::SizeOverflow)?
        .checked_mul(3)
        .ok_or(QoiEncodeError::SizeOverflow)?;

    if bytes.len() != expected_size {
        return Err(QoiEncodeError::InvalidInput);
    }

    unsafe {
        qoi_reserve_and_encode(bytes.as_ptr(), width, height, 3, out)
            .map(|_| ())
    }
}

/// Encode an RGBA image provided by the `bytes` into the `out` Vec.
///
/// `bytes` must hold exactly `width * height * 4` bytes.
#[inline]
pub fn qoi_encode_rgba_bytes(
    bytes: &[u8], width: u32, height: u32, out: &mut Vec<u8>,
) -> Result<(), QoiEncodeError> {
    let expected_size = usize::try_from(width as u64 * height as u64)
        .map_err(|_| QoiEncodeError::SizeOverflow)?
        .checked_mul(4)
        .ok_or(QoiEncodeError::SizeOverflow)?;

    if bytes.len() != expected_size {
        return Err(QoiEncodeError::InvalidInput);
    }

    unsafe {
        qoi_reserve_and_encode(bytes.as_ptr(), width, height, 3, out)
            .map(|_| ())
    }
}

/// Encodes an RGB image from the `image` crate.
#[cfg(feature = "image")]
#[inline]
pub fn qoi_encode_rgb_image(
    img: &image::RgbImage, out: &mut Vec<u8>,
) -> Result<usize, QoiEncodeError> {
    unsafe {
        qoi_reserve_and_encode(img.as_ptr(), img.width(), img.height(), 3, out)
    }
}

/// Encodes an RGBA image from the `image` crate.
#[cfg(feature = "image")]
#[inline]
pub fn qoi_encode_rgba_image(
    img: &image::RgbaImage, out: &mut Vec<u8>,
) -> Result<usize, QoiEncodeError> {
    unsafe {
        qoi_reserve_and_encode(img.as_ptr(), img.width(), img.height(), 4, out)
    }
}
