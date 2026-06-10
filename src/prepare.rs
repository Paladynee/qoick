use std::mem;

use crate::encode::qoi_encode_ch3;
use crate::encode::qoi_encode_ch4;
use crate::public::QoiEncodeError;

pub(crate) trait BufferInfo {
    fn data_ptr(&self) -> *const u8;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
}

impl BufferInfo for image::RgbaImage {
    #[inline]
    fn data_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    #[inline]
    fn width(&self) -> u32 {
        self.width()
    }

    #[inline]
    fn height(&self) -> u32 {
        self.height()
    }
}

impl BufferInfo for image::RgbImage {
    #[inline]
    fn data_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    #[inline]
    fn width(&self) -> u32 {
        self.width()
    }

    #[inline]
    fn height(&self) -> u32 {
        self.height()
    }
}

#[inline(never)]
pub(crate) fn qoi_prepare_encode(
    img: &dyn BufferInfo, channels: u8, out: &mut Vec<u8>,
) -> Result<usize, QoiEncodeError> {
    let width = img.width();
    let height = img.height();

    // get the output vector to stack memory so we can pass it into functions
    // quickly within registers. we don't want double pointer indirection to
    // the underlying output buffer, which optimizes really poorly.
    let mut scratch = vec![];
    mem::swap(&mut scratch, out);

    qoi_reserve(channels, width, height, &mut scratch)?;

    // SAFETY: BufferInfo is only implemented for image::RgbImage and
    // image::RgbaImage, whose pointers guarantee they point to at least
    // `size_of::<Pixel>() * img.width() * img.height()` bytes of data.
    let res = unsafe {
        let res = if channels == 3 {
            // SAFETY: above, `if preallocate` reserved enough bytes
            // to fit the biggest possible QOI image. therefore, we can use
            // the Buffer that increments a pointer.
            qoi_encode_ch3(
                img.data_ptr(),
                width,
                height,
                scratch.as_mut_ptr(),
            )
        } else {
            // SAFETY: above, `if preallocate` reserved enough bytes
            // to fit the biggest possible QOI image. therefore, we can use
            // the Buffer that increments a pointer.
            qoi_encode_ch4(
                img.data_ptr(),
                width,
                height,
                scratch.as_mut_ptr(),
            )
        };
        // SAFETY: above functions write exactly `res` bytes to the pointer.
        scratch.set_len(res);
        res
    };

    mem::swap(&mut scratch, out);
    // scratch is back to being an empty vec, no data is being leaked
    // here. we just remove the redundant drop for Vec that
    // can never get called.
    mem::forget(scratch);

    Ok(res)
}

pub static QOI_MAGIC: [u8; 4] = *b"qoif";
pub static QOI_PADDING: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

pub const QOI_HEADER_SIZE: usize =
    // size of "qoif"
    size_of_val(&QOI_MAGIC)
    // size of the width
    + size_of::<u32>()
    // size of the height
    + size_of::<u32>()
    // size of channels
    + size_of::<u8>()
    // size of colorspace
    + size_of::<u8>();
pub const QOI_PADDING_SIZE: usize = size_of_val(&QOI_PADDING);

#[inline]
fn qoi_reserve(
    channels: u8, w: u32, h: u32, out: &mut Vec<u8>,
) -> Result<(), QoiEncodeError> {
    /// Biggest possible representation of a QOI image is comprised of all
    /// QOI_OP_RGBA or QOI_OP_RGB pixels, depending on the colorspace. Each
    /// QOI_OP_RGBA takes 5 bytes (1 tag byte, 4 raw subpixel bytes), and each
    /// QOI_OP_RGB similarly takes 4 bytes to encode.
    ///
    /// This translates to the number `num_pixels + num_pixels * channels`
    /// bytes.
    #[inline]
    fn qoi_theoretical_max_size(channels: u8, num_pixels: u64) -> Option<u64> {
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

    let max_size = qoi_theoretical_max_size(channels, w as u64 * h as u64)
        .ok_or(QoiEncodeError::SizeOverflow)?;
    let reserve_size =
        isize::try_from(max_size).map_err(|_| QoiEncodeError::SizeOverflow)?;
    out.clear();
    out.reserve(reserve_size as usize);
    Ok(())
}
