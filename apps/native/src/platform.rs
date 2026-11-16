#[cfg(target_os = "macos")]
pub fn menu() {
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::NSString;
    let main = MainThreadMarker::new().expect("window event on main thread");
    let app = NSApplication::sharedApplication(main);
    let menu = app.mainMenu().expect("winit application menu");
    let application = menu
        .itemAtIndex(0)
        .and_then(|item| item.submenu())
        .expect("application submenu");
    let quit = application
        .itemAtIndex(application.numberOfItems() - 1)
        .expect("quit item");
    unsafe {
        quit.setAction(Some(sel!(performClose:)));
    }
    let edit = NSMenu::new(main);
    for (title, action, key) in [
        ("Cut", sel!(cut:), "x"),
        ("Copy", sel!(copy:), "c"),
        ("Paste", sel!(paste:), "v"),
        ("Select All", sel!(selectAll:), "a"),
    ] {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                main.alloc(),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        edit.addItem(&item);
    }
    let item = NSMenuItem::new(main);
    item.setTitle(&NSString::from_str("Edit"));
    item.setSubmenu(Some(&edit));
    menu.addItem(&item);
}
