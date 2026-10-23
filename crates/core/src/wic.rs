//! Image decoding through the Windows Imaging Component (WIC). Chosen in step 0: 3x faster
//! than the best pure-Rust path for thumbnails because its JPEG decoder can scale while
//! decoding (docs/bench-step0.md). Also covers PNG, GIF, BMP, TIFF and, with Microsoft's free
//! extensions, WebP/HEIC/RAW. EXIF orientation is NOT applied here; see `orient`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Result, anyhow};
use image::RgbImage;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::PCWSTR;

thread_local! {
    static FACTORY: Option<IWICImagingFactory> = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()
    };
}

fn with_factory<T>(f: impl FnOnce(&IWICImagingFactory) -> Result<T>) -> Result<T> {
    FACTORY.with(|fac| match fac {
        Some(fac) => f(fac),
        None => Err(anyhow!("Windows Imaging Component is unavailable")),
    })
}

fn frame(fac: &IWICImagingFactory, path: &Path) -> Result<IWICBitmapFrameDecode> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let dec = fac.CreateDecoderFromFilename(
            PCWSTR(wide.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        Ok(dec.GetFrame(0)?)
    }
}

pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    with_factory(|fac| unsafe {
        let fr = frame(fac, path)?;
        let (mut w, mut h) = (0, 0);
        fr.GetSize(&mut w, &mut h)?;
        Ok((w, h))
    })
}

/// Decodes so that the result fits inside `max_edge` (never enlarges). `None` = full size.
pub fn decode(path: &Path, max_edge: Option<u32>) -> Result<RgbImage> {
    with_factory(|fac| unsafe {
        let fr = frame(fac, path)?;
        let (mut w, mut h) = (0, 0);
        fr.GetSize(&mut w, &mut h)?;
        let (tw, th) = match max_edge {
            Some(m) if w.max(h) > m => fit(w, h, m),
            _ => (w, h),
        };
        let src: IWICBitmapSource = if (tw, th) != (w, h) {
            let sc = fac.CreateBitmapScaler()?;
            sc.Initialize(&fr, tw, th, WICBitmapInterpolationModeFant)?;
            sc.into()
        } else {
            fr.into()
        };
        let conv = fac.CreateFormatConverter()?;
        conv.Initialize(&src, &GUID_WICPixelFormat24bppRGB, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let stride = tw * 3;
        let mut buf = vec![0u8; (stride as usize) * (th as usize)];
        conv.CopyPixels(std::ptr::null(), stride, &mut buf)?;
        RgbImage::from_raw(tw, th, buf).ok_or_else(|| anyhow!("bad buffer size"))
    })
}

pub fn fit(w: u32, h: u32, edge: u32) -> (u32, u32) {
    if w >= h {
        (edge, ((h as f64 * edge as f64 / w as f64).round() as u32).max(1))
    } else {
        (((w as f64 * edge as f64 / h as f64).round() as u32).max(1), edge)
    }
}

/// Applies an EXIF orientation (1–8) so the picture is upright.
pub fn orient(img: RgbImage, orientation: i64) -> RgbImage {
    use image::imageops::*;
    match orientation {
        2 => flip_horizontal(&img),
        3 => rotate180(&img),
        4 => flip_vertical(&img),
        5 => flip_horizontal(&rotate90(&img)),
        6 => rotate90(&img),
        7 => flip_horizontal(&rotate270(&img)),
        8 => rotate270(&img),
        _ => img,
    }
}
