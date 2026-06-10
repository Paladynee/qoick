use criterion::{black_box, criterion_group, criterion_main, Criterion};
use image::ColorType;
use image::ImageEncoder;

use qoi;

fn load_rgba_image() -> image::RgbaImage {
    // adjust path if you want a different test image
    image::open("wife2.png").expect("failed to open test image").to_rgba8()
}

fn bench_qoi_encoders(c: &mut Criterion) {
    let img = load_rgba_image();

    c.bench_function("qoi_encode_rgba (local)", |b| {
        b.iter(|| {
            let mut out = Vec::new();
            qoi::public::qoi_encode_rgba(&img, &mut out, true).unwrap();
            black_box(&out);
        })
    });

    c.bench_function("image crate qoi encoder", |b| {
        b.iter(|| {
            let mut out = Vec::new();
            let mut encoder = image::codecs::qoi::QoiEncoder::new(&mut out);
            encoder
                .write_image(img.as_raw(), img.width(), img.height(), ColorType::Rgba8.into())
                .unwrap();
            black_box(&out);
        })
    });
}

criterion_group!(benches, bench_qoi_encoders);
criterion_main!(benches);
