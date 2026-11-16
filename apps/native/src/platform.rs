#[derive(Clone, Debug)]
pub enum ClipboardContent {
    Text(String),
    Png(Vec<u8>),
}

pub async fn read_clipboard() -> Result<Option<ClipboardContent>, String> {
    tokio::task::spawn_blocking(|| {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        let image = match clipboard.get_image() {
            Ok(image) => image,
            Err(arboard::Error::ContentNotAvailable) => {
                return match clipboard.get_text() {
                    Ok(text) => Ok(Some(ClipboardContent::Text(text))),
                    Err(arboard::Error::ContentNotAvailable) => Ok(None),
                    Err(error) => Err(error.to_string()),
                };
            }
            Err(error) => return Err(error.to_string()),
        };
        let width = u32::try_from(image.width).map_err(|error| error.to_string())?;
        let height = u32::try_from(image.height).map_err(|error| error.to_string())?;
        let mut png = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut png),
            &image.bytes,
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
        Ok(Some(ClipboardContent::Png(png)))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(target_os = "macos")]
pub fn menu() -> iced::Task<crate::Message> {
    use iced::futures::channel::mpsc::{UnboundedSender, unbounded};
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel,
    };
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::{NSObject, NSString};

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = UnboundedSender<()>]
        struct AImanityMenuActions;

        impl AImanityMenuActions {
            #[unsafe(method(uninstallAImanity:))]
            fn uninstall(&self, _item: &NSMenuItem) {
                let _ = self.ivars().unbounded_send(());
            }
        }
    );

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
    let (sender, receiver) = unbounded();
    let actions: Retained<AImanityMenuActions> = unsafe {
        msg_send![
            super(main.alloc::<AImanityMenuActions>().set_ivars(sender)),
            init
        ]
    };
    let uninstall = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            main.alloc(),
            &NSString::from_str("Uninstall AImanity…"),
            Some(sel!(uninstallAImanity:)),
            &NSString::from_str(""),
        )
    };
    unsafe {
        uninstall.setTarget(Some(&actions));
        uninstall.setRepresentedObject(Some(&actions));
    }
    application.insertItem_atIndex(&uninstall, application.numberOfItems() - 1);
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
    iced::Task::run(receiver, |()| crate::Message::Uninstall)
}
