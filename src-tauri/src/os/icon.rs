//! Icon-extraction seam (ADR-0005).
//!
//! The Windows implementation pulls an executable's associated icon via Shell +
//! GDI and converts it to RGBA. All `unsafe` FFI lives here in the os/ seam
//! (threat-model: "unsafe confined to os/"); every handle is checked and freed.
//! The pixel/alpha composition — the tricky part — is factored into the pure,
//! cross-platform [`compose_rgba`] and unit-tested, so the FFI is only thin glue.
//! Consumed by `art::exe_icon` to cache PNG covers for local games.
//!
//! NOTE: the live extraction (visual output) is verified manually on Windows
//! against real executables — like the other os/ real impls, it isn't unit-tested
//! (the fake serves logic tests).

use crate::error::CoreResult;
use std::path::Path;

/// A decoded icon as tightly-packed, top-down RGBA8 (`width * height * 4` bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Extracts an executable's associated icon as RGBA pixels.
pub trait IconExtractor {
    /// Extract `exe`'s icon. `Ok(None)` when it has none extractable.
    fn extract(&self, exe: &Path) -> CoreResult<Option<IconRgba>>;
}

/// Compose top-down RGBA8 from a GDI 32bpp top-down BGRA color buffer plus an
/// optional 32bpp AND-mask buffer (read as a DIB: `0` = opaque, non-zero =
/// transparent). When the color buffer already carries a non-zero alpha channel
/// (modern 32-bit icons), the mask is ignored. Returns `None` on a size
/// mismatch. Pure + cross-platform so the alpha logic is unit-tested.
pub(crate) fn compose_rgba(
    color_bgra: &[u8],
    mask_bgra: Option<&[u8]>,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    let bytes = pixels.checked_mul(4)?;
    if color_bgra.len() < bytes {
        return None;
    }
    let has_alpha = color_bgra[..bytes].chunks_exact(4).any(|px| px[3] != 0);
    // Keep the mask only if it's big enough; bound once, reuse in the loop.
    let mask = mask_bgra.filter(|m| m.len() >= bytes);

    let mut out = vec![0u8; bytes];
    for i in 0..pixels {
        let (b, g, r, a) = (
            color_bgra[i * 4],
            color_bgra[i * 4 + 1],
            color_bgra[i * 4 + 2],
            color_bgra[i * 4 + 3],
        );
        out[i * 4] = r;
        out[i * 4 + 1] = g;
        out[i * 4 + 2] = b;
        out[i * 4 + 3] = if has_alpha {
            a
        } else if let Some(mask) = mask {
            // AND-mask: 0 → opaque, non-zero → transparent.
            if mask[i * 4] == 0 {
                255
            } else {
                0
            }
        } else {
            255
        };
    }
    Some(out)
}

#[cfg(windows)]
pub use windows_impl::WindowsIcons;

#[cfg(windows)]
mod windows_impl {
    use super::{compose_rgba, IconExtractor, IconRgba};
    use crate::error::CoreResult;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    /// Largest icon dimension we accept (defensive; system icons are ≤256).
    const MAX_DIM: i32 = 512;

    pub struct WindowsIcons;

    impl IconExtractor for WindowsIcons {
        fn extract(&self, exe: &Path) -> CoreResult<Option<IconRgba>> {
            // SAFETY: standard Shell/GDI icon extraction. The HICON from
            // SHGetFileInfoW is destroyed before return; GDI bitmap handles and
            // the screen DC obtained inside `icon_to_rgba` are released there.
            // Pixel buffers are sized from the dimensions GetObjectW reports.
            unsafe {
                let wide: Vec<u16> = exe
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut info = SHFILEINFOW::default();
                let ret = SHGetFileInfoW(
                    PCWSTR(wide.as_ptr()),
                    FILE_ATTRIBUTE_NORMAL,
                    Some(&mut info),
                    std::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON,
                );
                if ret == 0 || info.hIcon.is_invalid() {
                    return Ok(None);
                }
                let result = icon_to_rgba(info.hIcon);
                let _ = DestroyIcon(info.hIcon);
                Ok(result)
            }
        }
    }

    /// Convert an HICON to [`IconRgba`]. Returns `None` (rather than erroring) for
    /// any unexpected shape — a missing cover is non-fatal. Frees the icon's
    /// color/mask bitmaps and the screen DC it allocates.
    unsafe fn icon_to_rgba(hicon: HICON) -> Option<IconRgba> {
        let mut ii = ICONINFO::default();
        if GetIconInfo(hicon, &mut ii).is_err() {
            return None;
        }
        let hbm_color = ii.hbmColor;
        let hbm_mask = ii.hbmMask;
        let result = (|| {
            if hbm_color.is_invalid() {
                return None; // monochrome icon — skip
            }
            let mut bmp = BITMAP::default();
            let n = GetObjectW(
                HGDIOBJ(hbm_color.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut bmp as *mut BITMAP).cast()),
            );
            if n == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
                return None;
            }
            if bmp.bmWidth > MAX_DIM || bmp.bmHeight > MAX_DIM {
                return None;
            }
            let (w, h) = (bmp.bmWidth, bmp.bmHeight);
            let pixels = (w as usize) * (h as usize);

            let mut header = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // negative → top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let mut bmi = BITMAPINFO {
                bmiHeader: header,
                ..Default::default()
            };

            let hdc = GetDC(None);
            if hdc.is_invalid() {
                return None;
            }

            let mut color = vec![0u8; pixels * 4];
            let got = GetDIBits(
                hdc,
                hbm_color,
                0,
                h as u32,
                Some(color.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            // Mask (for legacy icons without a real alpha channel).
            let mut mask = vec![0u8; pixels * 4];
            let mask_got = if !hbm_mask.is_invalid() {
                // Reset header (GetDIBits may have mutated it).
                header.biHeight = -h;
                bmi.bmiHeader = header;
                GetDIBits(
                    hdc,
                    hbm_mask,
                    0,
                    h as u32,
                    Some(mask.as_mut_ptr().cast()),
                    &mut bmi,
                    DIB_RGB_COLORS,
                )
            } else {
                0
            };

            ReleaseDC(None, hdc);

            if got == 0 {
                return None;
            }
            let mask_ref = (mask_got != 0).then_some(mask.as_slice());
            let rgba = compose_rgba(&color, mask_ref, w as u32, h as u32)?;
            Some(IconRgba {
                width: w as u32,
                height: h as u32,
                rgba,
            })
        })();

        if !hbm_color.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(hbm_color.0));
        }
        if !hbm_mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
        }
        result
    }
}

#[cfg(test)]
pub use fake::FakeIcons;

#[cfg(test)]
mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// In-memory icon source for tests: maps an exe path to its icon.
    #[derive(Default)]
    pub struct FakeIcons {
        icons: HashMap<PathBuf, IconRgba>,
    }

    impl FakeIcons {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_icon(mut self, exe: impl Into<PathBuf>, icon: IconRgba) -> Self {
            self.icons.insert(exe.into(), icon);
            self
        }
    }

    impl IconExtractor for FakeIcons {
        fn extract(&self, exe: &Path) -> CoreResult<Option<IconRgba>> {
            Ok(self.icons.get(exe).cloned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compose_rgba;

    #[test]
    fn compose_uses_color_alpha_when_present_and_swaps_bgra_to_rgba() {
        // One pixel: B=10 G=20 R=30 A=200.
        let color = [10u8, 20, 30, 200];
        let out = compose_rgba(&color, None, 1, 1).unwrap();
        assert_eq!(out, vec![30u8, 20, 10, 200]); // R,G,B,A
    }

    #[test]
    fn compose_falls_back_to_mask_when_alpha_all_zero() {
        // Two pixels, no alpha in color; mask: px0 opaque (0), px1 transparent (1).
        let color = [10u8, 20, 30, 0, 40, 50, 60, 0];
        let mask = [0u8, 0, 0, 0, 255, 255, 255, 255];
        let out = compose_rgba(&color, Some(&mask), 2, 1).unwrap();
        assert_eq!(out[3], 255, "px0 opaque via mask");
        assert_eq!(out[7], 0, "px1 transparent via mask");
    }

    #[test]
    fn compose_defaults_opaque_without_alpha_or_mask() {
        let color = [1u8, 2, 3, 0];
        let out = compose_rgba(&color, None, 1, 1).unwrap();
        assert_eq!(out[3], 255);
    }

    #[test]
    fn compose_rejects_size_mismatch() {
        assert!(compose_rgba(&[1, 2, 3], None, 1, 1).is_none());
    }
}
