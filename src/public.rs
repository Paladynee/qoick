use image::DynamicImage;

use crate::prepare::qoi_prepare_encode;

#[derive(Debug)]
pub enum QoiEncodeError {
    /// Provided image file can not possibly fit in the address space of RAM. It
    /// usually means your image dimensions are malformed, or the bigger than
    /// any image representable by your address space.
    SizeOverflow,
}

#[inline] // this is just a match, it should be inlined.
pub fn qoi_encode(
    img: &image::DynamicImage, out: &mut Vec<u8>, preallocate: bool,
) -> Result<usize, QoiEncodeError> {
    match img {
        DynamicImage::ImageRgb8(rgb) => qoi_encode_rgb(rgb, out, preallocate),
        DynamicImage::ImageRgba8(rgba) =>
            qoi_encode_rgba(rgba, out, preallocate),
        _ =>
            if img.has_alpha() {
                qoi_encode_rgba(&img.to_rgba8(), out, preallocate)
            } else {
                qoi_encode_rgb(&img.to_rgb8(), out, preallocate)
            },
    }
}

#[inline]
pub fn qoi_encode_rgb(
    img: &image::RgbImage, out: &mut Vec<u8>, preallocate: bool,
) -> Result<usize, QoiEncodeError> {
    qoi_prepare_encode(img, 3, out, preallocate)
}

#[inline]
pub fn qoi_encode_rgba(
    img: &image::RgbaImage, out: &mut Vec<u8>, preallocate: bool,
) -> Result<usize, QoiEncodeError> {
    qoi_prepare_encode(img, 4, out, preallocate)
}
