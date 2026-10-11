//! Route AppKit Quit through the editor's cancellable window-close path.
//!
//! eframe 0.35 / winit 0.30 has no public application-termination hook. Adding
//! this missing method preserves winit's delegate and its lifecycle callbacks.
//! Recheck this small compatibility bridge when upgrading either dependency.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, ffi, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply, NSView, NSWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::RefCell;
use std::io;

thread_local! {
    static ROOT_WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

extern "C-unwind" fn should_terminate(
    _: &AnyObject,
    _: Sel,
    application: &NSApplication,
) -> NSApplicationTerminateReply {
    let window = ROOT_WINDOW.with(|window| window.borrow().clone());
    if let Some(window) = window {
        application.unhide(None);
        window.deminiaturize(None);
        window.makeKeyAndOrderFront(None);
        window.performClose(None);
    }
    // AppKit must keep its normal event loop running for the editor's modal.
    // eframe exits after the resulting CloseRequested is accepted by the app.
    NSApplicationTerminateReply::TerminateCancel
}

pub fn install(
    cc: &eframe::CreationContext<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("Quit handling requires the main thread"))?;
    let RawWindowHandle::AppKit(handle) = cc.window_handle()?.as_raw() else {
        return Err(io::Error::other("OpenCad did not receive an AppKit window").into());
    };
    // The borrowed native view is valid for cc's window-handle lifetime; retain
    // its window so the callback always targets this editor, including when a
    // file panel is key or the editor is minimized.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let window = view
        .window()
        .ok_or_else(|| io::Error::other("OpenCad's native view has no window"))?;
    let application = NSApplication::sharedApplication(mtm);
    let delegate = application
        .delegate()
        .ok_or_else(|| io::Error::other("AppKit has no application delegate"))?;
    let delegate_object: &AnyObject = (*delegate).as_ref();
    let class = delegate_object.class();
    let selector = sel!(applicationShouldTerminate:);
    if class.name() != c"WinitApplicationDelegate" || class.instance_method(selector).is_some() {
        return Err(io::Error::other(
            "The application delegate changed; quit compatibility needs review",
        )
        .into());
    }
    ROOT_WINDOW.with(|root| *root.borrow_mut() = Some(window));
    // On both supported macOS ABIs this method returns NSUInteger (Q), takes
    // self (@), _cmd (:), and an NSApplication object (@). class_addMethod adds
    // only the absent method; no existing implementation or ivars are changed.
    let added = unsafe {
        let implementation: Imp = std::mem::transmute(
            should_terminate
                as extern "C-unwind" fn(
                    &AnyObject,
                    Sel,
                    &NSApplication,
                ) -> NSApplicationTerminateReply,
        );
        ffi::class_addMethod(
            (class as *const objc2::runtime::AnyClass).cast_mut(),
            selector,
            implementation,
            c"Q@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        ROOT_WINDOW.with(|root| *root.borrow_mut() = None);
        return Err(io::Error::other("Could not install cancellable AppKit quit handling").into());
    }
    Ok(())
}
