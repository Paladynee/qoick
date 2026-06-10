use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;

mod encode;
mod prepare;
mod public;

use image::DynamicImage;
use walkdir::WalkDir;

use crate::public::qoi_encode;

extern crate image;

fn main() {
    // // uncomment to test the optimized hash function
    // // on big endian and little endian
    // use crate::encode::Pixel;
    // use crate::encode::hash_pixel;
    // let pix = Pixel([1, 2, 3, 4]);
    // let hash1 = hash_pixel(&pix);
    // let actual1 = (pix[0] as u16 * 3
    //     + pix[1] as u16 * 5
    //     + pix[2] as u16 * 7
    //     + pix[3] as u16 * 11)
    //     % 64;

    // let pix = Pixel([1, 2, 3]);
    // let hash2 = hash_pixel(&pix);
    // let actual2 =
    //     (pix[0] as u16 * 3 + pix[1] as u16 * 5 + pix[2] as u16 * 7 + 255 *
    // 11) % 64;
    // dbg!(hash1, actual1);
    // dbg!(hash2, actual2);
    // return;
    let supported_image_formats = image::ImageFormat::all()
        .map(|imgf| (imgf.reading_enabled(), imgf.writing_enabled(), imgf))
        .filter(|(r, w, _)| *r || *w);
    for (reading, writing, imgf) in supported_image_formats {
        println!(
            "Image format: {:?}, Reading enabled: {}, Writing enabled: {}",
            imgf.extensions_str(),
            reading,
            writing
        );
    }

    let mut in_file = OsString::new();
    let mut out_file = OsString::new();
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_encoded_bytes() {
            b"-i" | b"--input" =>
                if let Some(file) = args.next() {
                    in_file = file;
                } else {
                    panic!("missing argument for -i/--input");
                },
            b"-o" | b"--output" =>
                if let Some(file) = args.next() {
                    out_file = file;
                } else {
                    panic!("missing argument for -o/--output");
                },
            // #[cfg(debug_assertions)]
            b"tests" => {
                let walkdir =
                    WalkDir::new("./test_images").min_depth(1).max_depth(1);
                for dirent in walkdir.into_iter().map(Result::unwrap) {
                    if dirent.file_type().is_file() {
                        let path = dirent.path();
                        if let Some(b"png") =
                            path.extension().map(|x| x.as_encoded_bytes())
                        {
                            let img =
                                image::open(path).expect("test file broken");
                            eprintln!("read test {}", path.display());
                            let mut v = vec![];
                            qoi_encode(&img, &mut v, true)
                                .expect("encode failed");
                            let mut out_path = path.to_owned();
                            out_path.set_extension("qoi");
                            let mut out = File::create(&out_path).unwrap();
                            out.write_all(&v).expect("io error");
                            eprintln!(
                                "test for {} written: {} bytes",
                                path.display(),
                                v.len()
                            );
                        }
                    }
                }
                return;
            }
            _ => {
                panic!("unknown argument: {:?}", arg);
            }
        }
    }
    if in_file.is_empty() {
        panic!("input file not specified");
    }

    let image = image::open(&in_file).expect("failed to open input image");
    let mut out_file = BufWriter::new(
        File::create(&out_file).expect("failed to create output file"),
    );

    let kind = match image {
        DynamicImage::ImageLuma8(_) => "ImageLuma8",
        DynamicImage::ImageLumaA8(_) => "ImageLumaA8",
        DynamicImage::ImageRgb8(_) => "ImageRgb8",
        DynamicImage::ImageRgba8(_) => "ImageRgba8",
        DynamicImage::ImageLuma16(_) => "ImageLuma16",
        DynamicImage::ImageLumaA16(_) => "ImageLumaA16",
        DynamicImage::ImageRgb16(_) => "ImageRgb16",
        DynamicImage::ImageRgba16(_) => "ImageRgba16",
        DynamicImage::ImageRgb32F(_) => "ImageRgb32F",
        DynamicImage::ImageRgba32F(_) => "ImageRgba32F",
        _ => "Unknown",
    };
    eprintln!(
        "[INFO] loaded image file {}\n\
        kind: {}\n\
        colorspace = {:?}\n\
        has alpha = {}\n\
        dimensions = {}x{}\n",
        in_file.display(),
        kind,
        image.color(),
        image.has_alpha(),
        image.width(),
        image.height(),
    );
    // image.save("wife2.png").unwrap();
    // todo: RGB and RGBA mode
    let mut v = vec![];
    qoi_encode(&image, &mut v, true).expect("failed to encode image");
    out_file.write_all(&v).expect("failed to write output file");
}
