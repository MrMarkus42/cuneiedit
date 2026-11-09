//! Printing on Windows: the system print dialog (printer, pages, copies) and printing text
//! with GDI. These Win32 calls need a few lines of `unsafe`.
//!
//! On other platforms this crate is empty.
#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt as _;
use std::ptr;

use windows_sys::Win32::Foundation::{GlobalFree, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontW, DEFAULT_CHARSET, DeleteDC, DeleteObject, FIXED_PITCH,
    FW_NORMAL, GET_DEVICE_CAPS_INDEX, GetDeviceCaps, GetTextExtentPoint32W, HDC, HFONT, HORZRES,
    LOGPIXELSX, LOGPIXELSY, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, TextOutW, VERTRES,
};
use windows_sys::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows_sys::Win32::UI::Controls::Dialogs::{
    PD_NOCURRENTPAGE, PD_NOSELECTION, PD_PAGENUMS, PD_RESULT_PRINT, PD_RETURNDC,
    PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PRINTPAGERANGE, PrintDlgExW, START_PAGE_GENERAL,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

/// Points per inch.
const POINTS: i32 = 72;
const FONT_POINTS: i32 = 9;
const FOOTER_POINTS: i32 = 8;
/// Page margin in hundredths of an inch.
const MARGIN: i32 = 70;
const MAX_RANGES: usize = 16;

/// A run of text and its color as RGB.
pub type Run = (String, [u8; 3]);

/// What the printer can hold with the editor font.
#[derive(Clone, Copy, Debug)]
pub struct PageSize {
    pub columns: usize,
    pub rows: usize,
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().collect()
}

fn wide_nul(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}

fn color(rgb: [u8; 3]) -> u32 {
    u32::from(rgb[0]) | u32::from(rgb[1]) << 8 | u32::from(rgb[2]) << 16
}

/// Shows the print dialog and prints the pages that `layout` makes for the chosen printer.
/// `footer` gets the page number and count. Returns `Ok(false)` if the user cancelled.
pub fn print(
    title: &str,
    layout: impl FnOnce(PageSize) -> Vec<Vec<Vec<Run>>>,
    footer: impl Fn(usize, usize) -> String,
) -> Result<bool, String> {
    let mut ranges = [PRINTPAGERANGE {
        nFromPage: 0,
        nToPage: 0,
    }; MAX_RANGES];
    let mut dialog = PRINTDLGEXW {
        lStructSize: size_of::<PRINTDLGEXW>() as u32,
        // SAFETY: no arguments; returns the active window of this (the UI) thread or null.
        hwndOwner: unsafe { GetActiveWindow() },
        Flags: PD_RETURNDC | PD_NOSELECTION | PD_NOCURRENTPAGE | PD_USEDEVMODECOPIESANDCOLLATE,
        nMaxPageRanges: MAX_RANGES as u32,
        lpPageRanges: ranges.as_mut_ptr(),
        nMinPage: 1,
        nMaxPage: 9999,
        nCopies: 1,
        nStartPage: START_PAGE_GENERAL,
        ..Default::default()
    };
    // SAFETY: `dialog` is fully initialized with its size; `lpPageRanges` points to
    // `nMaxPageRanges` entries that outlive the call.
    let result = unsafe { PrintDlgExW(&mut dialog) };
    // SAFETY: the dialog allocated these handles (or left them null); they are freed once.
    unsafe {
        GlobalFree(dialog.hDevMode);
        GlobalFree(dialog.hDevNames);
    }
    if result < 0 {
        return Err(format!("the print dialog failed (0x{result:08X})"));
    }
    let hdc = dialog.hDC;
    if dialog.dwResultAction != PD_RESULT_PRINT || hdc.is_null() {
        if !hdc.is_null() {
            // SAFETY: a device context returned by the dialog, deleted once.
            unsafe { DeleteDC(hdc) };
        }
        return Ok(false);
    }
    let chosen = (dialog.Flags & PD_PAGENUMS != 0)
        .then(|| ranges[..dialog.nPageRanges.min(MAX_RANGES as u32) as usize].to_vec());
    let result = print_pages(hdc, title, layout, footer, chosen.as_deref());
    // SAFETY: the device context from the dialog, no longer used.
    unsafe { DeleteDC(hdc) };
    result.map(|()| true)
}

/// A device capability of the printer `hdc`.
fn caps(hdc: HDC, index: GET_DEVICE_CAPS_INDEX) -> i32 {
    // SAFETY: `hdc` is a valid printer device context; the call only reads it.
    unsafe { GetDeviceCaps(hdc, index as i32) }
}

/// A font of `points` size for the printer `hdc`; the caller deletes it.
fn font(hdc: HDC, points: i32) -> HFONT {
    let face = wide_nul("Consolas");
    let height = -(points * caps(hdc, LOGPIXELSY) / POINTS);
    // SAFETY: plain values and `face`, which is NUL-terminated and outlives the call.
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            0,
            0,
            u32::from(CLEARTYPE_QUALITY),
            u32::from(FIXED_PITCH),
            face.as_ptr(),
        )
    }
}

fn text_size(hdc: HDC, text: &[u16]) -> SIZE {
    let mut size = SIZE { cx: 0, cy: 0 };
    // SAFETY: `text` is valid for its length and `size` is a valid out pointer.
    unsafe { GetTextExtentPoint32W(hdc, text.as_ptr(), text.len() as i32, &mut size) };
    size
}

fn print_pages(
    hdc: HDC,
    title: &str,
    layout: impl FnOnce(PageSize) -> Vec<Vec<Vec<Run>>>,
    footer: impl Fn(usize, usize) -> String,
    ranges: Option<&[PRINTPAGERANGE]>,
) -> Result<(), String> {
    let (width, height, dpi_x, dpi_y) = (
        caps(hdc, HORZRES),
        caps(hdc, VERTRES),
        caps(hdc, LOGPIXELSX),
        caps(hdc, LOGPIXELSY),
    );
    let (margin_x, margin_y) = (dpi_x * MARGIN / 100, dpi_y * MARGIN / 100);
    let text_font = font(hdc, FONT_POINTS);
    let footer_font = font(hdc, FOOTER_POINTS);
    if text_font.is_null() || footer_font.is_null() {
        return Err("no font for the printer".to_owned());
    }
    // SAFETY: valid context and font; the previous font is restored before deleting ours.
    let previous = unsafe { SelectObject(hdc, text_font) };
    let cell = text_size(hdc, &wide("M"));
    let line_height = (cell.cy * 115 / 100).max(1);
    let size = PageSize {
        columns: ((width - 2 * margin_x) / cell.cx.max(1)).max(1) as usize,
        rows: ((height - 2 * margin_y - 2 * line_height) / line_height).max(1) as usize,
    };
    let pages = layout(size);
    let total = pages.len();
    let selected = |page: usize| {
        ranges.is_none_or(|ranges| {
            ranges
                .iter()
                .any(|r| (r.nFromPage as usize..=r.nToPage as usize).contains(&page))
        })
    };
    let name = wide_nul(title);
    let info = DOCINFOW {
        cbSize: size_of::<DOCINFOW>() as i32,
        lpszDocName: name.as_ptr(),
        lpszOutput: ptr::null(),
        lpszDatatype: ptr::null(),
        fwType: 0,
    };
    let result = (|| {
        // SAFETY: `info` and the title it points to outlive the call.
        if unsafe { StartDocW(hdc, &info) } <= 0 {
            return Err("the printer did not accept the document".to_owned());
        }
        for (index, page) in pages.iter().enumerate() {
            if !selected(index + 1) {
                continue;
            }
            // SAFETY: inside a started document on a valid context.
            if unsafe { StartPage(hdc) } <= 0 {
                return Err("the printer did not accept a page".to_owned());
            }
            // SAFETY: valid context; GDI resets the selected objects with each page on some
            // drivers, so the font and mode are set again.
            unsafe {
                SelectObject(hdc, text_font);
                SetBkMode(hdc, TRANSPARENT as i32);
            }
            for (row_index, row) in page.iter().enumerate() {
                let y = margin_y + row_index as i32 * line_height;
                let mut x = margin_x;
                for (text, rgb) in row {
                    let text = wide(text);
                    // SAFETY: `text` is valid for its length; plain GDI drawing calls.
                    unsafe {
                        SetTextColor(hdc, color(*rgb));
                        TextOutW(hdc, x, y, text.as_ptr(), text.len() as i32);
                    }
                    x += text_size(hdc, &text).cx;
                }
            }
            let footer = wide(&footer(index + 1, total));
            // SAFETY: as above; the footer font belongs to this context.
            unsafe {
                SelectObject(hdc, footer_font);
                SetTextColor(hdc, 0);
                TextOutW(
                    hdc,
                    margin_x,
                    height - margin_y,
                    footer.as_ptr(),
                    footer.len() as i32,
                );
            }
            // SAFETY: ends the page started above.
            if unsafe { EndPage(hdc) } <= 0 {
                return Err("printing a page failed".to_owned());
            }
        }
        // SAFETY: ends the document started above.
        unsafe { EndDoc(hdc) };
        Ok(())
    })();
    // SAFETY: restores the context's font, then deletes ours, which are no longer selected;
    // an unfinished document is cancelled.
    unsafe {
        if result.is_err() {
            AbortDoc(hdc);
        }
        SelectObject(hdc, previous);
        DeleteObject(text_font);
        DeleteObject(footer_font);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_colors_and_text() {
        assert_eq!(color([0x12, 0x34, 0x56]), 0x0056_3412);
        assert_eq!(wide_nul("ä"), vec![0xE4, 0]);
    }
}
