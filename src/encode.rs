use core::hint::assert_unchecked;
use core::ops::Deref;
use core::ops::DerefMut;
use core::ptr;
use core::slice;

/// Represents a preallocated unsafe buffer.
#[repr(transparent)]
pub(crate) struct Buffer {
    pub ptr: *mut u8,
}

impl Buffer {
    #[inline(always)]
    fn push(&mut self, val: u8) {
        unsafe {
            self.ptr.write(val);
            self.ptr = self.ptr.add(1);
        }
    }

    #[inline(always)]
    fn extend_from_array<const N: usize>(&mut self, slice: [u8; N]) {
        unsafe {
            ptr::copy_nonoverlapping(slice.as_ptr(), self.ptr, N);
            self.ptr = self.ptr.add(N);
        }
    }
}

pub static QOI_MAGIC: [u8; 4] = *b"qoif";
pub static QOI_PADDING: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

const QOI_OP_INDEX: u8 = 0b00000000;
const QOI_OP_DIFF: u8 = 0b01000000;
const QOI_OP_LUMA: u8 = 0b10000000;
const QOI_OP_RUN: u8 = 0b11000000;
const QOI_OP_RGB: u8 = 0b11111110;
const QOI_OP_RGBA: u8 = 0b11111111;

#[repr(align(64))]
struct CacheAligned<T>(pub T);

impl<T> Deref for CacheAligned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for CacheAligned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The everything function. This is used ONLY for monomorphizing the 2
/// versions of QOI for two different channel configurations. It could be
/// seen like a type safe macro.
#[inline(always)]
pub(crate) unsafe fn qoi_encode_generic<const CHANNELS: usize>(
    image_ptr: *const u8, width: u32, height: u32, mut out: Buffer,
) -> Buffer {
    debug_assert!(CHANNELS == 3 || CHANNELS == 4);
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

    // @perf: 18% perf win by cache aligning these
    let mut prev_pix = CacheAligned(const { Pixel::<CHANNELS>::new() });
    let mut lookup = CacheAligned([const { Pixel::<CHANNELS>::zero() }; 64]);
    let mut hash = CacheAligned(0u8);
    // -1: no run yet, 0: run of length 1, range -1..=61.
    // serialize `run` directly. no adding 1 needed anywhere.
    let mut run = CacheAligned(-1i8);

    // the QOI specification specifies the order of these implicitly like
    // this:
    // • a run of the previous pixel
    // • an index into an array of previously seen pixels
    // • a difference to the previous pixel value in r,g,b
    // • full r,g,b or r,g,b,a values

    // the reason behind following it is that when encoding a run of just 2
    // pixels, if we don't enforce the order we have 2 ways to encode that:
    // - a raw pixel + run of length 1
    // - a raw pixel + some index into last seen array
    // the index into the last seen array has 6 bits that change based on
    // the pixel hash, whereas "1 run" has just 1 possible fixed bit
    // pattern. therefore, if QOI is wrapped in a general purpose compressor
    // it should yield more predictability at the cost of nothing.
    //
    // note that this only applies to 2 pixel runs. any other amount of runs
    // are not affected.
    for pix in pixels.iter().cloned() {
        // • a run of the previous pixel
        if pix == prev_pix.0 {
            run.0 += 1;
            if run.0 == 61 {
                // emitting run of length 62. this is easily const propagated
                // to 0b11111101.
                out.push(QOI_OP_RUN | 61);
                // no need to set prev_pix or write into lookup, since
                // runs of the same pixel don't change either of these.

                run.0 = -1;
            }
            continue;
        }

        if run.0 != -1 {
            // emitting run of length run + 1
            // we can just serialize run directly
            out.push(QOI_OP_RUN | run.0 as u8);
            // no need to set prev_pix or write into lookup, since
            // runs of the same pixel don't change either of these.

            run.0 = -1;
        }

        // • an index into an array of previously seen pixels

        hash.0 = hash_pixel::<CHANNELS>(&pix);
        // help optimizer in case it doesn't see the `& 63` across
        // the function call boundary
        unsafe {
            assert_unchecked(hash.0 < 64);
        }

        if pix == lookup[hash.0 as usize] {
            // emitting index into lookup table
            out.push(QOI_OP_INDEX | hash.0);
            prev_pix.0 = pix;
            // no need to write into lookup, since the pixel is proven to exist
            // in the lookup table.

            continue;
        }

        // • a difference to the previous pixel value in r,g,b
        if CHANNELS == 4 && pix[3] != prev_pix[3] {
            // get the diff
            let (dr, mut dg, mut db) = (
                pix[0].wrapping_sub(prev_pix[0]) as i8,
                pix[1].wrapping_sub(prev_pix[1]) as i8,
                pix[2].wrapping_sub(prev_pix[2]) as i8,
            );
            match (dr, dg, db) {
                // if -2..=1, store.
                (-2..=1, -2..=1, -2..=1) => {
                    // storage: 0b00 = -2, 0b11 = 1
                    // so you just add 2.
                    // let mut buf = QOI_OP_DIFF;
                    // these additions never overflow/underflow as
                    // they're in range
                    // buf |= ((dr.wrapping_add(2)) << 4) as u8;
                    // buf |= ((dg.wrapping_add(2)) << 2) as u8;
                    // buf |= (db.wrapping_add(2)) as u8;

                    // reuse existing locals
                    // @perf: zero impact. could remove in the future
                    db = db.wrapping_add(2);
                    db |= (dg.wrapping_add(2)) << 2;
                    db |= (dr.wrapping_add(2)) << 4;
                    db |= QOI_OP_DIFF as i8;

                    // out.push(buf);
                    out.push(db as u8);
                    prev_pix.0 = pix;
                    lookup[hash.0 as usize] = pix;

                    continue;
                }
                _ => {
                    // @perf: this condition gains us ~1.3% perf
                    if matches!(dg, -32..=31) {
                        let drg = dr.wrapping_sub(dg);
                        let mut dbg = db.wrapping_sub(dg);

                        if matches!((dg, drg, dbg), (-32..=31, -8..=7, -8..=7))
                        {
                            // let mut buf1 = QOI_OP_LUMA;
                            // buf1 |= (dg.wrapping_add(32)) as u8;
                            // let mut buf2 = 0u8;
                            // buf2 |= (drg.wrapping_add(8) << 4) as u8;
                            // buf2 |= (dbg.wrapping_add(8)) as u8;

                            // out.extend_from_array([buf1, buf2]);

                            // reuse existing locals
                            // @perf: zero impact. could remove in the future
                            dg = dg.wrapping_add(32);
                            dg |= QOI_OP_LUMA as i8;

                            dbg = dbg.wrapping_add(8);
                            dbg |= drg.wrapping_add(8) << 4;

                            out.extend_from_array([dg as u8, dbg as u8]);

                            prev_pix.0 = pix;
                            lookup[hash.0 as usize] = pix;

                            continue;
                        }
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

        prev_pix.0 = pix;
        lookup[hash.0 as usize] = pix;
    }

    // • a run of the previous pixel
    // runs that end up exhausting the pixels.
    if run.0 != -1 {
        // emitting a run of length run + 1
        out.push(QOI_OP_RUN | run.0 as u8);
    }

    out.extend_from_array(QOI_PADDING); // end marker

    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct Pixel<const N: usize>(pub [u8; N]);

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

// from the rust crate `qoi`: github.com/aldanor/qoi-rust
// credits for the initial idea: @zakarumych
#[inline]
pub(crate) fn hash_pixel<const N: usize>(pix: &Pixel<N>) -> u8 {
    let v = if N < 4 {
        u32::from_le_bytes([pix[0], pix[1], pix[2], 255])
    } else {
        u32::from_le_bytes([pix[0], pix[1], pix[2], pix[3]])
    } as u64;
    let s = ((v & 0xff00_ff00) << 32) | (v & 0x00ff_00ff);
    (s.wrapping_mul(0x0300_0700_0005_000b_u64) >> 56) as u8 & 63
}
