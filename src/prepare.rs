use alloc::vec::Vec;

use crate::QoiEncodeError;
use crate::encode::QOI_MAGIC;
use crate::encode::QOI_PADDING;
use crate::qoi_encode_buf_size;
use crate::qoi_raw_encode_3_channels;
use crate::qoi_raw_encode_4_channels;

#[inline(never)]
pub(crate) unsafe fn qoi_reserve_and_encode(
    data: *const u8, w: u32, h: u32, channels: u8, out: &mut Vec<u8>,
) -> Result<usize, QoiEncodeError> {
    qoi_reserve(channels, w, h, out)?;

    // SAFETY: this function is only called by image::RgbImage and
    // image::RgbaImage, whose pointers guarantee they point to at least
    // `size_of::<Pixel>() * w * h` bytes of data.
    let res = unsafe {
        let res = if channels == 3 {
            // SAFETY: above, `if preallocate` reserved enough bytes
            // to fit the biggest possible QOI image. therefore, we can use
            // the Buffer that increments a pointer.
            qoi_raw_encode_3_channels(data, w, h, out.as_mut_ptr())
        } else {
            // SAFETY: above, `if preallocate` reserved enough bytes
            // to fit the biggest possible QOI image. therefore, we can use
            // the Buffer that increments a pointer.
            qoi_raw_encode_4_channels(data, w, h, out.as_mut_ptr())
        };
        // SAFETY: above functions write exactly `res` bytes to the pointer.
        out.set_len(res);
        res
    };

    Ok(res)
}

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
    let max_size = qoi_encode_buf_size(channels, w, h)
        .ok_or(QoiEncodeError::SizeOverflow)?;
    let reserve_size =
        isize::try_from(max_size).map_err(|_| QoiEncodeError::SizeOverflow)?;
    out.clear();
    out.reserve(reserve_size as usize);
    Ok(())
}
