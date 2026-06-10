use core::slice;
use std::hint::assert_unchecked;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr;

use crate::prepare::QOI_MAGIC;
use crate::prepare::QOI_PADDING;

struct Buffer<const BOUNDS_CHECK: bool>
where
    Self: Pusher,
{
    data: <Self as Pusher>::Data,
}

trait Pusher {
    type Data;
    fn push(&mut self, val: u8);
    fn extend_from_array<const N: usize>(&mut self, slice: [u8; N]);
}

impl Pusher for Buffer<true> {
    type Data = Vec<u8>;

    #[inline(always)]
    fn push(&mut self, val: u8) {
        self.data.push(val);
    }

    #[inline(always)]
    fn extend_from_array<const N: usize>(&mut self, slice: [u8; N]) {
        self.data.extend_from_slice(&slice);
    }
}

impl Pusher for Buffer<false> {
    type Data = *mut u8;

    #[inline(always)]
    fn push(&mut self, val: u8) {
        unsafe {
            self.data.write(val);
            self.data = self.data.add(1);
        }
    }

    #[inline(always)]
    fn extend_from_array<const N: usize>(&mut self, slice: [u8; N]) {
        unsafe {
            ptr::copy_nonoverlapping(slice.as_ptr(), self.data, N);
            self.data = self.data.add(N);
        }
    }
}

#[inline(never)]
pub(crate) unsafe fn qoi_encode_ch4_preallocated(
    image_ptr: *const u8, width: u32, height: u32, out_ptr: *mut u8,
) -> usize {
    unsafe {
        qoi_encode_generic::<4, false>(image_ptr, width, height, Buffer {
            data: out_ptr,
        })
        .data
        .addr()
            - out_ptr.addr()
    }
}

#[inline(never)]
pub(crate) unsafe fn qoi_encode_ch3_preallocated(
    image_ptr: *const u8, width: u32, height: u32, out_ptr: *mut u8,
) -> usize {
    unsafe {
        let res =
            qoi_encode_generic::<3, false>(image_ptr, width, height, Buffer {
                data: out_ptr,
            })
            .data;

        println!("res = {res:p}\nimg = {image_ptr:p}");
        res.addr() - out_ptr.addr()
    }
}

#[inline(never)]
pub(crate) unsafe fn qoi_encode_ch4_vec(
    image_ptr: *const u8, width: u32, height: u32, out: Vec<u8>,
) -> Vec<u8> {
    unsafe {
        qoi_encode_generic::<4, true>(image_ptr, width, height, Buffer {
            data: out,
        })
        .data
    }
}

#[inline(never)]
pub(crate) unsafe fn qoi_encode_ch3_vec(
    image_ptr: *const u8, width: u32, height: u32, out: Vec<u8>,
) -> Vec<u8> {
    unsafe {
        qoi_encode_generic::<3, true>(image_ptr, width, height, Buffer {
            data: out,
        })
        .data
    }
}

#[expect(unused)]
pub const QOI_OP_INDEX: u8 = 0b00000000;
pub const QOI_OP_DIFF: u8 = 0b01000000;
pub const QOI_OP_LUMA: u8 = 0b10000000;
pub const QOI_OP_RUN: u8 = 0b11000000;
pub const QOI_OP_RGB: u8 = 0b11111110;
pub const QOI_OP_RGBA: u8 = 0b11111111;

/// The everything function. This is used ONLY for monomorphizing the 4
/// versions of this function. It could be seen like a type safe macro.
#[inline(always)]
unsafe fn qoi_encode_generic<const CHANNELS: usize, const PUSH_ALLOCS: bool>(
    // invariant: must point to at least width * height * CHANNELS bytes.
    image_ptr: *const u8,
    width: u32,
    height: u32,
    mut out: Buffer<PUSH_ALLOCS>,
) -> Buffer<PUSH_ALLOCS>
where
    Buffer<PUSH_ALLOCS>: Pusher,
{
    assert!(CHANNELS == 3 || CHANNELS == 4);

    out.extend_from_array(QOI_MAGIC);
    out.extend_from_array(width.to_be_bytes());
    out.extend_from_array(height.to_be_bytes());
    out.push(CHANNELS as u8);
    out.push(0);

    let pixels: &[Pixel<CHANNELS>] = unsafe {
        slice::from_raw_parts(
            image_ptr.cast(),
            width as usize * height as usize,
        )
    };

    let mut prev_pix = const { Pixel::<CHANNELS>::new() };
    let mut hash;
    let mut lookup = [const { Pixel::<CHANNELS>::zero() }; 64];
    // -1: no run yet, 0: run of length 1, range -1..=61.
    // serialize `run` directly. no adding 1 needed anywhere.
    let mut run = -1i8;

    // the QOI specification specifies the order of these implicitly like
    // this:
    // • a run of the previous pixel
    // • an index into an array of previously seen pixels
    // • a difference to the previous pixel value in r,g,b
    // • full r,g,b or r,g,b,a values

    // the reason behind following it is that when encoding a run of just 2
    // pixels, if we don't enforce the order we have 2 ways to encode that:
    // - a raw pixel + run of length 0
    // - a raw pixel + some index into last seen array
    // the index into the last seen array has 6 bits that change based on
    // the pixel hash, whereas "1 run" has just 1 possible fixed bit
    // pattern. therefore, if QOI is wrapped in a general purpose compressor
    // it should yield more predictability at the cost of nothing.
    for pix in pixels.iter().copied() {
        // • a run of the previous pixel
        if pix == prev_pix {
            run += 1;
            if run == 61 {
                // emitting run of length 62. this is easily const propagated
                out.push(QOI_OP_RUN | 61);
                // no need to set prev_pix or write into lookup, since
                // runs of the same pixel don't change either of these.

                run = -1;
            }
            continue;
        }

        if run != -1 {
            // emitting run of length run + 1
            // we can just serialize run directly
            out.push(QOI_OP_RUN | run as u8);
            // no need to set prev_pix or write into lookup, since
            // runs of the same pixel don't change either of these.

            run = -1;
        }

        // • an index into an array of previously seen pixels

        hash = hash_pixel::<CHANNELS>(&pix);
        unsafe {
            assert_unchecked(hash < 64);
        }

        if pix == lookup[hash as usize] {
            // emitting index into lookup table
            // we don't need to QOI_OP_INDEX | index_position as u8
            // since QOI_OP_INDEX is 0, and index position is strictly < 64.
            out.push(hash);
            prev_pix = pix;
            // no need to write into lookup, since the pixel is proven to exist
            // in the lookup table.

            continue;
        }

        // • a difference to the previous pixel value in r,g,b
        if CHANNELS == 4 && pix[3] != prev_pix[3] {
            // get the diff
            let (dr, dg, db) = (
                pix[0].wrapping_sub(prev_pix[0]) as i8,
                pix[1].wrapping_sub(prev_pix[1]) as i8,
                pix[2].wrapping_sub(prev_pix[2]) as i8,
            );
            match (dr, dg, db) {
                // if -2..=1, store.
                (-2..=1, -2..=1, -2..=1) => {
                    // storage: 0b00 = -2, 0b11 = 1
                    // so you just add 2.
                    let mut buf = QOI_OP_DIFF;
                    // these additions never overflow/underflow as
                    // they're in range
                    buf |= ((dr.wrapping_add(2)) << 4) as u8;
                    buf |= ((dg.wrapping_add(2)) << 2) as u8;
                    buf |= (db.wrapping_add(2)) as u8;

                    out.push(buf);
                    prev_pix = pix;
                    lookup[hash as usize] = pix;

                    continue;
                }
                _ => {
                    // todo @perf: early return dg, does it increase
                    // performance?
                    let drg = dr.wrapping_sub(dg);
                    let dbg = db.wrapping_sub(dg);

                    if matches!((dg, drg, dbg), (-32..=31, -8..=7, -8..=7)) {
                        let mut buf1 = QOI_OP_LUMA;
                        buf1 |= (dg.wrapping_add(32)) as u8;
                        let mut buf2 = 0u8;
                        buf2 |= (drg.wrapping_add(8) << 4) as u8;
                        buf2 |= (dbg.wrapping_add(8)) as u8;

                        out.extend_from_array([buf1, buf2]);
                        prev_pix = pix;
                        lookup[hash as usize] = pix;

                        continue;
                    }
                }
            }
        }

        // • full r,g,b or r,g,b,a values

        // we can either reach this by too much diff bitween pixels or having
        // too much diff between diff between pixels.
        // emitting full RGB/RGBA pixel
        if CHANNELS == 4 {
            out.extend_from_array([
                QOI_OP_RGBA,
                pix[0],
                pix[1],
                pix[2],
                pix[3],
            ]);
        } else {
            out.extend_from_array([QOI_OP_RGB, pix[0], pix[1], pix[2]]);
        }

        lookup[hash as usize] = pix;
        prev_pix = pix;
    }

    // • a run of the previous pixel
    // runs that end up exhausting the pixels.
    if run != -1 {
        // emitting a run of length run + 1
        out.push(QOI_OP_RUN | run as u8);
    }

    out.extend_from_array(QOI_PADDING); // end marker

    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Pixel<const N: usize>(pub [u8; N]);

impl<const N: usize> Pixel<N> {
    #[inline]
    pub const fn new() -> Self {
        Self::with_alpha(255)
    }

    #[inline]
    pub const fn zero() -> Self {
        Pixel([0; N])
    }

    #[inline]
    pub const fn with_alpha(alpha: u8) -> Self {
        if N == 4 {
            let mut dat = [0; N];
            dat[3] = alpha;
            Pixel(dat)
        } else {
            Pixel([0; N])
        }
    }
}

impl<const N: usize> Deref for Pixel<N> {
    type Target = [u8; N];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> DerefMut for Pixel<N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// from the rust crate `qoi`: github.com/aldanor/qoi-rust
// credits for the initial idea: @zakarumych
#[inline]
pub fn hash_pixel<const N: usize>(pix: &Pixel<N>) -> u8 {
    let v = if N == 3 {
        u32::from_le_bytes([pix[0], pix[1], pix[2], 255])
    } else if N == 4 {
        u32::from_le_bytes([pix[0], pix[1], pix[2], pix[3]])
    } else {
        panic!("static always panic: N must be 3 or 4");
    } as u64;
    let s = ((v & 0xff00_ff00) << 32) | (v & 0x00ff_00ff);
    (s.wrapping_mul(0x0300_0700_0005_000b_u64) >> 56) as u8 & 63
}

// fn qoi_encode(img: image::RgbaImage, out: &mut Vec<u8>) -> io::Result<()> {
//     out.extend_from_slice(b"qoif");
//     let w = img.width();
//     let h = img.height();
//     out.extend_from_slice(&w.to_be_bytes());
//     out.extend_from_slice(&h.to_be_bytes());
//     // todo: support RGB mode
//     let channels = 4;
//     out.push(channels);
//     let colorspace = 0; // 0 for sRGB with linear alpha, 1 for all channels
// linear     out.push(colorspace);

//     let mut index_position: u32;
//     let mut prev_pixel = Rgba::<u8>([0, 0, 0, 255]);
//     let mut array = [const { Rgba::<u8>([0, 0, 0, 0]) }; 64];
//     let mut run_length: i8 = -1;

//     #[expect(unused)]
//     pub const QOI_OP_INDEX: u8 = 0b00000000;
//     pub const QOI_OP_DIFF: u8 = 0b01000000;
//     pub const QOI_OP_LUMA: u8 = 0b10000000;
//     pub const QOI_OP_RUN: u8 = 0b11000000;
//     pub const QOI_OP_RGB: u8 = 0b11111110;
//     pub const QOI_OP_RGBA: u8 = 0b11111111;

//     for pix in img.pixels().copied() {
//         if pix == prev_pixel {
//             run_length += 1;
//             if run_length == 61 {
//                 // emitting QOI_OP_RUN 62
//                 out.push(QOI_OP_RUN | 61);
//                 run_length = -1;
//             }
//             continue;
//         }
//         if run_length != -1 {
//             // emitting QOI_OP_RUN run_length + 1
//             out.push(QOI_OP_RUN | run_length as u8);
//             run_length = -1;
//         }
//         // index_position = (pix[0] as u32 * 3
//         //     + pix[1] as u32 * 5
//         //     + pix[2] as u32 * 7
//         //     + pix[3] as u32 * 11)
//         //     % 64;
//         index_position = hash_pixel(pix) as _;
//         unsafe {
//             assert_unchecked(index_position < 64);
//         }
//         if pix == array[index_position as usize] {
//             // emitting QOI_OP_INDEX
//             // we don't need to QOI_OP_INDEX | index_position as u8
//             // since QOI_OP_INDEX is 0, and index position is strictly < 64.
//             out.push(index_position as u8);
//             prev_pixel = pix;
//             continue;
//         }

//         if pix[3] != prev_pixel[3] {
//             let dr = pix[0].wrapping_sub(prev_pixel[0]) as i8;
//             let dg = pix[1].wrapping_sub(prev_pixel[1]) as i8;
//             let db = pix[2].wrapping_sub(prev_pixel[2]) as i8;
//             match (dr, dg, db) {
//                 (-2..=1, -2..=1, -2..=1) => {
//                     // emit QOI_OP_DIFF
//                     let mut buf: u8 = 0b01000000;
//                     buf |= ((dr as u8) << 4) & 0b00110000;
//                     buf |= ((dg as u8) << 2) & 0b00001100;
//                     buf |= ((db as u8) << 0) & 0b00000011;
//                     out.push(buf);
//                     array[index_position as usize] = pix;
//                     prev_pixel = pix;
//                     continue;
//                 }
//                 (-32..=31, ..) => {
//                     let drdg = dr - dg;
//                     let dbdg = db - dg;
//                     if (-8..=7).contains(&drdg) && (-8..=7).contains(&dbdg) {
//                         // emit QOI_OP_LUMA
//                         let mut buf1: u8 = 0b10000000;
//                         buf1 |= (dg as u8) & 0b00111111;

//                         let mut buf2: u8 = 0b00000000;
//                         buf2 |= ((drdg as u8) << 4) & 0b11110000;
//                         buf2 |= ((dbdg as u8) << 0) & 0b00001111;
//                         out.extend_from_slice(&[buf1, buf2]);
//                         array[index_position as usize] = pix;
//                         prev_pixel = pix;
//                         continue;
//                     }
//                 }
//                 _ => {}
//             }
//         }

//         // we can either reach this by too much diff bitween pixels or having
//         // too much diff between diff between pixels.
//         // emitting QOI_OP_RGBA
//         out.extend_from_slice(&[0b11111111, pix[0], pix[1], pix[2], pix[3]]);
//         array[index_position as usize] = pix;
//         prev_pixel = pix;
//     }

//     // runs that end up exhausting the iterator
//     if run_length != -1 {
//         // emitting QOI_OP_RUN run_lengh + 1
//         out.push(0b11000000 | run_length as u8);
//     }

//     out.extend_from_slice(&1u64.to_be_bytes()); // end marker
//     Ok(())
// }

// fn hash_pixel(pix: Rgba<u8>) -> u8 {
//     // from the rust crate `qoi`:
//     // credits for the initial idea: @zakarumych
//     let v = u32::from_ne_bytes(pix.0) as u64;
//     let s = ((v & 0xff00_ff00) << 32) | (v & 0x00ff_00ff);
//     s.wrapping_mul(0x0300_0700_0005_000b_u64)
//         .to_le()
//         .swap_bytes() as u8
//         & 63
// }
