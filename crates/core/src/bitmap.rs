use crate::Error;
pub const WIDTH: usize = 792;
pub const HEIGHT: usize = 528;
pub const PIXELS: usize = WIDTH * HEIGHT;
pub const PLANE_BYTES: usize = PIXELS / 8;

fn length(pixels: usize, bpp: u8) -> Result<usize, Error> {
    if pixels == 0 || pixels > PIXELS || !matches!(bpp, 1 | 2) {
        return Err(Error::Invalid);
    }
    Ok((pixels * bpp as usize).div_ceil(8))
}
/// MSB-first row-major samples. Zero is black; maximum is white.
pub fn pack(samples: &[u8], bpp: u8, out: &mut [u8]) -> Result<usize, Error> {
    let n = length(samples.len(), bpp)?;
    if out.len() < n {
        return Err(Error::BufferTooSmall);
    }
    if samples.iter().any(|&x| x >= (1 << bpp)) {
        return Err(Error::Invalid);
    }
    out[..n].fill(0);
    for (i, &x) in samples.iter().enumerate() {
        let bit = i * bpp as usize;
        out[bit / 8] |= x << (8 - bpp as usize - bit % 8);
    }
    Ok(n)
}
pub fn unpack(packed: &[u8], bpp: u8, samples: &mut [u8]) -> Result<(), Error> {
    let n = length(samples.len(), bpp)?;
    if packed.len() != n {
        return Err(Error::Invalid);
    }
    let used = samples.len() * bpp as usize % 8;
    if used != 0 && packed[n - 1] & ((1 << (8 - used)) - 1) != 0 {
        return Err(Error::Invalid);
    }
    for (i, x) in samples.iter_mut().enumerate() {
        let bit = i * bpp as usize;
        *x = (packed[bit / 8] >> (8 - bpp as usize - bit % 8)) & ((1 << bpp) - 1);
    }
    Ok(())
}
/// Ordered 4×4 Bayer dithering, deterministic and without row buffers.
pub fn dither(grey: &[u8], width: usize, bpp: u8, out: &mut [u8]) -> Result<(), Error> {
    length(grey.len(), bpp)?;
    if width == 0
        || !grey.len().is_multiple_of(width)
        || !((width <= WIDTH && grey.len() / width <= HEIGHT)
            || (width <= HEIGHT && grey.len() / width <= WIDTH))
    {
        return Err(Error::Invalid);
    }
    if out.len() != grey.len() {
        return Err(Error::BufferTooSmall);
    }
    const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let levels = (1i32 << bpp) - 1;
    for (i, (&v, dst)) in grey.iter().zip(out.iter_mut()).enumerate() {
        // Midpoints are distributed evenly; endpoints remain exact.
        let threshold = (2 * BAYER[i / width % 4][i % width % 4] + 1) * 255;
        *dst = ((i32::from(v) * levels * 32 + 8160 - threshold) / 8160).clamp(0, levels) as u8;
    }
    Ok(())
}
