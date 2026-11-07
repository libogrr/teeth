mod common;
use common::*;
use std::io::Cursor;
use teeth::wav::{build_header, header_len, WavFormat, WavWriter};

#[test]
fn float_stereo_one_frame_layout() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let mut w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    w.write_audio(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    let b = w.finish().unwrap().into_inner();
    assert_eq!(b.len(), 124);
    assert_eq!(&b[0..4], b"RIFF"); assert_eq!(u32le(&b, 4), 116); assert_eq!(&b[8..12], b"WAVE");
    assert_eq!(&b[12..16], b"JUNK"); assert_eq!(u32le(&b, 16), 28); assert!(b[20..48].iter().all(|&x| x == 0));
    assert_eq!(&b[48..52], b"fmt "); assert_eq!(u32le(&b, 52), 40); assert_eq!(&b[56..96], f.fmt_chunk());
    assert_eq!(&b[96..100], b"fact"); assert_eq!(u32le(&b, 100), 4); assert_eq!(u32le(&b, 104), 1);
    assert_eq!(&b[108..112], b"data"); assert_eq!(u32le(&b, 112), 8);
    assert_eq!(&b[116..124], &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn pcm16_has_no_fact() {
    let f = WavFormat::pcm(44_100, 2, 16);
    assert_eq!(header_len(&f), 80);
    let h = build_header(&f, 4, true);
    assert_eq!(&h[56..72], f.fmt_chunk());
    assert_eq!(&h[72..76], b"data"); assert_eq!(u32le(&h, 76), 4);
}

#[test]
fn odd_data_gets_pad_byte() {
    let f = WavFormat::pcm(48_000, 1, 24);
    let mut w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    w.write_audio(&[9; 9]).unwrap();
    let b = w.finish().unwrap().into_inner();
    assert_eq!(b.len(), 90);
    assert_eq!(u32le(&b, 4), 82);
    assert_eq!(u32le(&b, 76), 9);
    assert_eq!(b[89], 0);
}

#[test]
fn silence_uses_silence_byte() {
    let f = WavFormat::pcm(48_000, 1, 8);
    let mut w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    w.write_silence(3).unwrap();
    assert_eq!(w.frames(), 3);
    let b = w.finish().unwrap().into_inner();
    assert_eq!(&b[80..83], &[0x80; 3]);
}

#[test]
fn final_header_over_4gb_is_rf64() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let n = 5_000_000_000u64;
    let h = build_header(&f, n, true);
    assert_eq!(&h[0..4], b"RF64"); assert_eq!(u32le(&h, 4), u32::MAX);
    assert_eq!(&h[12..16], b"ds64"); assert_eq!(u32le(&h, 16), 28);
    assert_eq!(u64le(&h, 20), 108 + n);
    assert_eq!(u64le(&h, 28), n);
    assert_eq!(u64le(&h, 36), n / 8);
    assert_eq!(u32le(&h, 44), 0);
    assert_eq!(u32le(&h, 104), u32::MAX);
    assert_eq!(u32le(&h, 112), u32::MAX);
}

#[test]
fn provisional_header_over_4gb_is_clamped_riff() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let h = build_header(&f, 5_000_000_000, false);
    assert_eq!(&h[0..4], b"RIFF"); assert_eq!(u32le(&h, 4), u32::MAX);
    assert_eq!(&h[12..16], b"JUNK");
    assert_eq!(u32le(&h, 104), 625_000_000);
    assert_eq!(u32le(&h, 112), u32::MAX);
}

#[test]
fn update_header_keeps_appending() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let mut w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    w.write_audio(&[1; 8]).unwrap();
    w.update_header().unwrap();
    w.write_audio(&[2; 8]).unwrap();
    let b = w.finish().unwrap().into_inner();
    assert_eq!(u32le(&b, 112), 16);
    assert_eq!(&b[116..132], &[[1u8; 8], [2u8; 8]].concat()[..]);
}
