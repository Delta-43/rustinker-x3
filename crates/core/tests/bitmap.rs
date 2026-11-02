use x3_core::Error;
use x3_core::bitmap::*;
#[test]
fn round_trip_and_msb_order() {
    for bpp in [1, 2] {
        let samples = [0, 1, 0, 1];
        let mut bytes = [0; 1];
        let mut decoded = [0; 4];
        pack(&samples, bpp, &mut bytes).unwrap();
        if bpp == 1 {
            assert_eq!(bytes, [0x50]);
        }
        unpack(&bytes, bpp, &mut decoded).unwrap();
        assert_eq!(samples, decoded);
    }
}
#[test]
fn reject_bad_buffers_samples_padding_dimensions() {
    assert_eq!(pack(&[2], 1, &mut [0]), Err(Error::Invalid));
    assert!(pack(&[0], 3, &mut [0]).is_err());
    assert!(pack(&[0], 1, &mut []).is_err());
    assert!(unpack(&[1], 1, &mut [0]).is_err());
    assert!(unpack(&[], 1, &mut [0]).is_err());
    assert!(pack(&[], 1, &mut []).is_err());
    assert!(dither(&[0; 4], 3, 1, &mut [0; 4]).is_err());
    assert!(dither(&[0; 4], 0, 1, &mut [0; 4]).is_err());
    assert!(dither(&[0; 4], 4, 1, &mut []).is_err());
}
#[test]
fn bayer_golden_and_endpoints() {
    let mut out = [0; 16];
    dither(&[128; 16], 4, 1, &mut out).unwrap();
    assert_eq!(out, [1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1]);
    for bpp in [1, 2] {
        dither(&[0; 16], 4, bpp, &mut out).unwrap();
        assert_eq!(out, [0; 16]);
        dither(&[255; 16], 4, bpp, &mut out).unwrap();
        assert_eq!(out, [(1 << bpp) - 1; 16]);
    }
}
