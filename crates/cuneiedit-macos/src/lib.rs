//! Files that macOS asks CuneiEdit to open: "Open With" and double clicks in Finder, and files
//! dropped on the Dock icon; quit requests from the Dock or the system; and the system print
//! dialog. macOS sends the first two to the application delegate. winit owns that delegate
//! and does not forward them, so two methods are added to its class at runtime. These
//! Objective-C calls need a few lines of `unsafe`.
//!
//! On other platforms this crate is empty.
#![cfg(target_os = "macos")]

use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{AnyThread, MainThreadMarker, ffi, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply, NSPrintInfo};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};
use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};

/// Files received but not yet taken by the app.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// Set when macOS asked to quit, until the app takes it.
static QUIT: AtomicBool = AtomicBool::new(false);
/// Called when files or a quit request arrive, e.g. to wake the event loop.
type Waker = Box<dyn Fn() + Send>;
static WAKER: Mutex<Option<Waker>> = Mutex::new(None);

fn wake() {
    if let Some(wake) = WAKER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        wake();
    }
}

fn receive(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    OPENED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(paths);
    wake();
}

/// `-[delegate application:openURLs:]`
extern "C-unwind" fn open_urls(
    _this: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
    urls: &NSArray<NSURL>,
) {
    let paths = urls
        .iter()
        .filter(|url| url.isFileURL())
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect();
    receive(paths);
}

/// `-[delegate applicationShouldTerminate:]`: the app quits through its own path instead,
/// which keeps unsaved text.
extern "C-unwind" fn should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _application: &NSApplication,
) -> NSApplicationTerminateReply {
    QUIT.store(true, Ordering::Relaxed);
    wake();
    NSApplicationTerminateReply::TerminateCancel
}

/// Adds the handlers to the class of winit's application delegate. Call on the main thread
/// after the event loop was created and before it runs, so files that launched the app are
/// not missed.
pub fn install_open_handler() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        return;
    };
    let object: &AnyObject = delegate.as_ref();
    let class: *const AnyClass = object.class();
    type OpenUrls = extern "C-unwind" fn(&AnyObject, Sel, &NSApplication, &NSArray<NSURL>);
    type ShouldTerminate =
        extern "C-unwind" fn(&AnyObject, Sel, &NSApplication) -> NSApplicationTerminateReply;
    let methods: [(Sel, Imp, &CStr); 2] = [
        (
            sel!(application:openURLs:),
            // SAFETY: only the pointer type changes; the runtime calls it with these arguments.
            unsafe { std::mem::transmute::<OpenUrls, Imp>(open_urls) },
            c"v@:@@",
        ),
        (
            sel!(applicationShouldTerminate:),
            // SAFETY: as above; the reply is an NSUInteger, encoded as `Q`.
            unsafe { std::mem::transmute::<ShouldTerminate, Imp>(should_terminate) },
            c"Q@:@",
        ),
    ];
    for (selector, imp, types) in methods {
        // SAFETY: `class` is a registered class; the type encodings match the functions above.
        // An existing method is left alone (the call then returns false).
        unsafe {
            ffi::class_addMethod(class.cast_mut(), selector, imp, types.as_ptr());
        }
    }
}

/// Calls `wake` whenever files or a quit request arrive.
pub fn set_waker(wake: impl Fn() + Send + 'static) {
    *WAKER.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(wake));
}

/// Files macOS asked to open since the last call.
pub fn take_opened_files() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether macOS asked to quit (Dock, logout) since the last call.
pub fn take_quit_request() -> bool {
    QUIT.swap(false, Ordering::Relaxed)
}

/// Shows the system print dialog (with preview) for a PDF document, named `title` in the
/// print queue. Blocks until the dialog is closed. Returns false if the PDF is invalid.
pub fn print_pdf(pdf: &[u8], title: &str) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let data = NSData::with_bytes(pdf);
    // SAFETY: `data` is a valid NSData; content that is no PDF gives `None`.
    let Some(document) = (unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }) else {
        return false;
    };
    let info = NSPrintInfo::sharedPrintInfo();
    // SAFETY: valid print info; the operation only reads the document.
    let operation = unsafe {
        document.printOperationForPrintInfo_scalingMode_autoRotate(
            Some(&info),
            PDFPrintScalingMode::PageScaleDownToFit,
            true,
            mtm,
        )
    };
    let Some(operation) = operation else {
        return false;
    };
    operation.setJobTitle(Some(&NSString::from_str(title)));
    operation.setShowsPrintPanel(true);
    operation.setShowsProgressPanel(true);
    operation.runOperation();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_received_files() {
        receive(vec![PathBuf::from("/a.txt")]);
        receive(Vec::new());
        assert_eq!(take_opened_files(), vec![PathBuf::from("/a.txt")]);
        assert!(take_opened_files().is_empty());
    }
}
