mod common;
use teeth::wav::{SampleKind, WavError, WavFormat};

const IEEE_GUID: [u8; 16] = [3,0,0,0, 0,0, 0x10,0, 0x80,0,0,0xAA,0,0x38,0x9B,0x71];

#[test]
fn float32_extensible_fmt_bytes() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let mut want = vec![0xFE,0xFF, 2,0, 0x80,0xBB,0,0, 0x00,0xDC,0x05,0, 8,0, 32,0, 22,0, 32,0, 3,0,0,0];
    want.extend_from_slice(&IEEE_GUID);
    assert_eq!(f.fmt_chunk(), &want[..]);
    assert_eq!(f.sample_kind(), SampleKind::F32);
    assert!(f.needs_fact());
    assert_eq!(f.block_align(), 8);
    assert_eq!(f.describe(), "48000 Hz · 32-bit float · 2 ch");
}

#[test]
fn pcm16_fmt_is_16_bytes_without_fact() {
    let f = WavFormat::pcm(44_100, 2, 16);
    assert_eq!(f.fmt_chunk(), &[1,0, 2,0, 0x44,0xAC,0,0, 0x10,0xB1,0x02,0, 4,0, 16,0][..]);
    assert!(!f.needs_fact());
    assert_eq!(f.describe(), "44100 Hz · 16-bit · 2 ch");
}

#[test]
fn from_waveformatex_keeps_bytes_exactly() {
    let src = WavFormat::float32_extensible(96_000, 8);
    let back = WavFormat::from_waveformatex_bytes(src.fmt_chunk()).unwrap();
    assert_eq!(back.fmt_chunk(), src.fmt_chunk());
    assert_eq!((back.channels(), back.sample_rate(), back.block_align()), (8, 96_000, 32));
}

#[test]
fn rejects_unknown_subformat() {
    let mut b = WavFormat::float32_extensible(48_000, 2).fmt_chunk().to_vec();
    b[24] = 9; // primer byte del GUID de subformato
    assert!(matches!(WavFormat::from_waveformatex_bytes(&b), Err(WavError::Unsupported(_))));
}

#[test]
fn decode_scales_each_kind() {
    let mut out = vec![];
    WavFormat::pcm(48_000, 2, 16).decode_f32(&[0x00,0x80, 0x00,0x40], &mut out);
    assert_eq!(out, vec![-1.0, 0.5]);
    out.clear();
    WavFormat::pcm(48_000, 1, 24).decode_f32(&[0x00,0x00,0x80, 0xFF,0xFF,0x7F], &mut out);
    assert_eq!(out, vec![-1.0, 8_388_607.0 / 8_388_608.0]);
    out.clear();
    WavFormat::pcm(48_000, 1, 8).decode_f32(&[0x80], &mut out);
    assert_eq!(out, vec![0.0]);
    assert_eq!(WavFormat::pcm(48_000, 1, 8).silence_byte(), 0x80);
    out.clear();
    WavFormat::float32_extensible(48_000, 1).decode_f32(&1.5f32.to_le_bytes(), &mut out);
    assert_eq!(out, vec![1.5]);
}

#[test]
fn error_texts_are_in_spanish() {
    let io = std::io::Error::other("sin espacio");
    assert_eq!(WavError::Io(io).to_string(), "error de lectura/escritura: sin espacio");
    assert_eq!(WavError::NotWav.to_string(), "el archivo no es un WAV válido");
    let mut b = WavFormat::pcm(48_000, 2, 16).fmt_chunk().to_vec();
    b[14] = 12; // 12-bit PCM
    let e = WavFormat::from_waveformatex_bytes(&b).unwrap_err();
    assert_eq!(e.to_string(), "formato no compatible: PCM de 12 bits");
}

#[test]
fn with_rate_changes_only_the_rate_and_the_byte_rate() {
    for f in [WavFormat::pcm(48_000, 2, 16), WavFormat::float32_extensible(48_000, 2)] {
        let g = f.with_rate(40_363);
        let (a, b) = (f.fmt_chunk(), g.fmt_chunk());
        assert_eq!(a.len(), b.len());
        assert_eq!(g.sample_rate(), 40_363);
        assert_eq!(b[8..12], (40_363 * u32::from(f.block_align())).to_le_bytes());
        assert_eq!(a[..4], b[..4]);
        assert_eq!(a[12..], b[12..]);
    }
}

/// The bytes of `format`'s fmt chunk with `value` written at `offset`.
fn patched(format: &WavFormat, offset: usize, value: &[u8]) -> Vec<u8> {
    let mut bytes = format.fmt_chunk().to_vec();
    bytes[offset..offset + value.len()].copy_from_slice(value);
    bytes
}

#[test]
fn a_format_that_does_not_add_up_is_rejected() {
    for format in [WavFormat::pcm(48_000, 2, 16), WavFormat::float32_extensible(48_000, 2)] {
        let block = format.block_align();
        for bad in [
            patched(&format, 2, &0u16.to_le_bytes()),
            patched(&format, 4, &0u32.to_le_bytes()),
            patched(&format, 12, &0u16.to_le_bytes()),
            patched(&format, 12, &(block + 1).to_le_bytes()),
            patched(&format, 12, &(block / 2).to_le_bytes()),
        ] {
            let result = WavFormat::from_waveformatex_bytes(&bad);
            assert!(matches!(result, Err(WavError::Unsupported(_))), "{bad:?}");
        }
    }
}

#[test]
fn a_format_that_adds_up_is_accepted() {
    let formats = [
        WavFormat::pcm(44_100, 1, 8),
        WavFormat::pcm(44_100, 1, 24),
        WavFormat::pcm(192_000, 8, 32),
        WavFormat::float32_extensible(8_000, 6),
    ];
    for format in formats {
        assert!(WavFormat::from_waveformatex_bytes(format.fmt_chunk()).is_ok());
    }
}

#[test]
fn an_absurd_rate_does_not_overflow_the_byte_rate() {
    let g = WavFormat::float32_extensible(48_000, 2).with_rate(u32::MAX);
    assert_eq!(g.sample_rate(), u32::MAX);
    assert_eq!(g.fmt_chunk()[8..12], u32::MAX.to_le_bytes());
}
