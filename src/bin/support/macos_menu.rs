//! AppKit menus use the recorded CUI hierarchy and queue editor commands.

use eframe::egui;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu,
    NSMenuItem, NSMenuItemValidation,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use opencad::desktop::{MenuItem, menu_action_status, native_menus, queue_menu_action};
use std::cell::RefCell;
use std::io;

struct TargetState {
    ctx: egui::Context,
    actions: Vec<&'static str>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetState]
    struct OpenCadMenuTarget;

    unsafe impl NSObjectProtocol for OpenCadMenuTarget {}

    unsafe impl NSMenuItemValidation for OpenCadMenuTarget {
        #[unsafe(method(validateMenuItem:))]
        fn validate(&self, item: &NSMenuItem) -> bool {
            if let Some(action) = usize::try_from(item.tag()).ok().and_then(|index| self.ivars().actions.get(index)) {
                let status = menu_action_status(&self.ivars().ctx, action);
                item.setState(if status.checked == Some(true) { NSControlStateValueOn } else { NSControlStateValueOff });
                status.enabled
            } else {
                false
            }
        }
    }

    impl OpenCadMenuTarget {
        #[unsafe(method(runMenuAction:))]
        fn run(&self, item: &NSMenuItem) {
            if let Some(action) = usize::try_from(item.tag()).ok().and_then(|index| self.ivars().actions.get(index)) {
                queue_menu_action(&self.ivars().ctx, action);
            }
        }
    }
);

thread_local! {
    // NSMenuItem's target is weak. Keep it alive for the native app's lifetime.
    static TARGET: RefCell<Option<Retained<OpenCadMenuTarget>>> = const { RefCell::new(None) };
}

fn collect_actions(items: &'static [MenuItem], actions: &mut Vec<&'static str>) {
    for item in items {
        if !item.action.is_empty() && !actions.contains(&item.action) {
            actions.push(item.action);
        }
        collect_actions(item.children, actions);
    }
}

fn shortcut(action: &str) -> Option<(&'static str, NSEventModifierFlags)> {
    let command = NSEventModifierFlags::Command;
    Some(match action {
        "ID_New" => ("n", command),
        "ID_Open" => ("o", command),
        "ID_Save" => ("s", command),
        "ID_Saveas" => ("s", command | NSEventModifierFlags::Shift),
        "ID_DWG_CLOSE" => ("w", command),
        "ID_U" => ("z", command),
        "ID_Redo" => ("z", command | NSEventModifierFlags::Shift),
        "ID_SelAll" => ("a", command),
        "ID_Properties" => ("1", command),
        "ID_CmdLine" => ("3", command),
        "ID_ToolSets" => ("5", command),
        _ => return None,
    })
}

fn build_menu(
    mtm: MainThreadMarker,
    title: &str,
    items: &'static [MenuItem],
    target: &OpenCadMenuTarget,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setTitle(&NSString::from_str(title));
    menu.setAutoenablesItems(true);
    for entry in items {
        if entry.label.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        }
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(entry.label));
        if !entry.children.is_empty() {
            item.setSubmenu(Some(&build_menu(mtm, entry.label, entry.children, target)));
        } else if let Some(index) = target
            .ivars()
            .actions
            .iter()
            .position(|action| *action == entry.action)
        {
            item.setTag(index as isize);
            // The selector accepts exactly one NSMenuItem; TARGET retains self.
            unsafe {
                item.setTarget(Some(target.as_ref()));
                item.setAction(Some(sel!(runMenuAction:)));
            }
            if let Some((key, modifiers)) = shortcut(entry.action) {
                item.setKeyEquivalent(&NSString::from_str(key));
                item.setKeyEquivalentModifierMask(modifiers);
            }
            item.setEnabled(menu_action_status(&target.ivars().ctx, entry.action).enabled);
        } else if entry.label == "Minimize" || entry.label == "Zoom" {
            // Standard responder-chain actions apply to the front native window.
            unsafe {
                item.setAction(Some(if entry.label == "Minimize" {
                    sel!(performMiniaturize:)
                } else {
                    sel!(performZoom:)
                }));
            }
            if entry.label == "Minimize" {
                item.setKeyEquivalent(&NSString::from_str("m"));
                item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            }
        } else {
            item.setEnabled(false);
        }
        menu.addItem(&item);
    }
    menu
}

pub fn install(ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("Native menus require the main thread"))?;
    let application = NSApplication::sharedApplication(mtm);
    let root = application
        .mainMenu()
        .ok_or_else(|| io::Error::other("AppKit has no main menu"))?;
    let mut actions = Vec::new();
    collect_actions(native_menus(), &mut actions);
    let target = OpenCadMenuTarget::alloc(mtm).set_ivars(TargetState {
        ctx: ctx.clone(),
        actions,
    });
    // NSObject's init establishes the allocated instance; it adds no resources.
    let target: Retained<OpenCadMenuTarget> = unsafe { msg_send![super(target), init] };
    // Preserve winit's application menu, including Services, Hide, and Quit.
    while root.numberOfItems() > 1 {
        root.removeItemAtIndex(1);
    }
    for entry in native_menus() {
        let menu = build_menu(mtm, entry.label, entry.children, &target);
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(entry.label));
        item.setSubmenu(Some(&menu));
        root.addItem(&item);
        if entry.label == "Help" {
            application.setHelpMenu(Some(&menu));
        }
        if entry.label == "Window" {
            application.setWindowsMenu(Some(&menu));
        }
    }
    TARGET.with(|slot| *slot.borrow_mut() = Some(target));
    Ok(())
}
