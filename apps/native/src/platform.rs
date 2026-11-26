use iced::{Size, Task, window};

#[derive(Clone, Debug)]
pub enum ClipboardContent {
    Text(String),
    Png(Vec<u8>),
}

#[cfg(target_os = "macos")]
pub fn reduced_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(windows)]
pub fn reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut animation = 1_i32;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut animation as *mut i32).cast(),
            0,
        )
    };
    ok != 0 && animation == 0
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn reduced_motion() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn appkit_view(window: &dyn window::Window) -> &objc2_app_kit::NSView {
    use window::raw_window_handle::RawWindowHandle;

    let RawWindowHandle::AppKit(handle) = window.window_handle().expect("AppKit handle").as_raw()
    else {
        unreachable!("AppKit window")
    };
    unsafe { handle.ns_view.cast::<objc2_app_kit::NSView>().as_ref() }
}

#[cfg(target_os = "macos")]
pub fn lock_aspect(id: window::Id, aspect: Size) -> Task<()> {
    use objc2::{
        runtime::{AnyObject, Imp, Sel},
        sel,
    };
    use objc2_app_kit::NSWindow;
    use objc2_foundation::NSSize;

    window::run(id, move |window| {
        let window = appkit_view(window).window().expect("attached AppKit view");
        let object: &AnyObject = window.as_ref();
        // Winit resets resize increments at drag start and finish, which clears the ratio.
        type ResizeIncrements = unsafe extern "C-unwind" fn(&NSWindow, Sel, NSSize);
        unsafe {
            objc2::ffi::class_addMethod(
                std::ptr::from_ref(object.class()).cast_mut(),
                sel!(setContentResizeIncrements:),
                std::mem::transmute::<ResizeIncrements, Imp>(keep_content_aspect),
                c"v@:{CGSize=dd}".as_ptr(),
            );
        }
        window.setContentAspectRatio(NSSize::new(
            f64::from(aspect.width),
            f64::from(aspect.height),
        ));
    })
}

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn keep_content_aspect(
    window: &objc2_app_kit::NSWindow,
    _: objc2::runtime::Sel,
    size: objc2_foundation::NSSize,
) {
    use objc2::{ClassType, msg_send};
    use objc2_app_kit::NSWindow;

    if window.contentAspectRatio().height == 0. {
        let _: () = unsafe {
            msg_send![super(window, NSWindow::class()), setContentResizeIncrements: size]
        };
    }
}

#[cfg(target_os = "macos")]
pub fn content_size(id: window::Id) -> Task<Size> {
    window::run(id, |window| {
        let size = appkit_view(window).bounds().size;
        Size::new(size.width as f32, size.height as f32)
    })
}

#[cfg(windows)]
fn hwnd(window: &dyn window::Window) -> windows_sys::Win32::Foundation::HWND {
    use window::raw_window_handle::RawWindowHandle;

    let RawWindowHandle::Win32(handle) = window.window_handle().expect("Win32 handle").as_raw()
    else {
        unreachable!("Win32 window")
    };
    handle.hwnd.get() as _
}

#[cfg(windows)]
pub fn lock_aspect(id: window::Id, aspect: Size) -> Task<()> {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;

    let ratio = (aspect.width / aspect.height).to_bits() as usize;
    window::run(id, move |window| {
        assert_ne!(
            unsafe { SetWindowSubclass(hwnd(window), Some(size_workbench), 0, ratio) },
            0,
            "Win32 window sizing hook"
        );
    })
}

#[cfg(windows)]
pub fn content_size(id: window::Id) -> Task<Size> {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::GetClientRect},
    };

    window::run(id, |window| unsafe {
        let hwnd = hwnd(window);
        let mut rect = RECT::default();
        assert_ne!(GetClientRect(hwnd, &mut rect), 0, "Win32 client bounds");
        let scale = GetDpiForWindow(hwnd) as f32 / 96.;
        Size::new(rect.right as f32 / scale, rect.bottom as f32 / scale)
    })
}

#[cfg(windows)]
unsafe extern "system" fn size_workbench(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    edge: usize,
    lparam: isize,
    subclass: usize,
    data: usize,
) -> isize {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass},
            WindowsAndMessaging::{
                GetClientRect, GetWindowRect, WM_NCDESTROY, WM_SIZING, WMSZ_BOTTOM,
                WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOP, WMSZ_TOPLEFT,
                WMSZ_TOPRIGHT,
            },
        },
    };

    if message == WM_SIZING {
        let mut frame = RECT::default();
        let mut client = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut frame) != 0 && GetClientRect(hwnd, &mut client) != 0 }
        {
            let rect = unsafe { &mut *(lparam as *mut RECT) };
            let width =
                f64::from(rect.right - rect.left - (frame.right - frame.left - client.right));
            let height =
                f64::from(rect.bottom - rect.top - (frame.bottom - frame.top - client.bottom));
            let ratio = f64::from(f32::from_bits(data as u32));
            let edge = edge as u32;
            let (w, h) = match edge {
                WMSZ_LEFT | WMSZ_RIGHT => (width, width / ratio),
                WMSZ_TOP | WMSZ_BOTTOM => (height * ratio, height),
                _ => {
                    let h = (width * ratio + height) / (ratio * ratio + 1.);
                    (h * ratio, h)
                }
            };
            let resize = |low: &mut i32, high: &mut i32, delta: i32, start, end| {
                let from_start = if start {
                    delta
                } else if end {
                    0
                } else {
                    delta / 2
                };
                *low -= from_start;
                *high += delta - from_start;
            };
            resize(
                &mut rect.left,
                &mut rect.right,
                (w - width).round() as i32,
                matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT),
                matches!(edge, WMSZ_RIGHT | WMSZ_TOPRIGHT | WMSZ_BOTTOMRIGHT),
            );
            resize(
                &mut rect.top,
                &mut rect.bottom,
                (h - height).round() as i32,
                matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT),
                matches!(edge, WMSZ_BOTTOM | WMSZ_BOTTOMLEFT | WMSZ_BOTTOMRIGHT),
            );
            return 1;
        }
    } else if message == WM_NCDESTROY {
        unsafe { RemoveWindowSubclass(hwnd, Some(size_workbench), subclass) };
    }
    unsafe { DefSubclassProc(hwnd, message, edge, lparam) }
}

#[cfg(target_os = "linux")]
pub fn lock_aspect(id: window::Id, aspect: Size) -> Task<()> {
    use window::raw_window_handle::RawWindowHandle;
    use x11rb::properties::{AspectRatio, WmSizeHints};

    window::run(id, move |window| {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let window_id = match handle.as_raw() {
            RawWindowHandle::Xlib(handle) => handle.window as u32,
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            _ => return,
        };
        let apply = || -> Result<(), Box<dyn std::error::Error>> {
            let (connection, _) = x11rb::connect(None)?;
            let mut hints = WmSizeHints::get_normal_hints(&connection, window_id)?
                .reply()?
                .unwrap_or_default();
            let ratio = AspectRatio::new(aspect.width as i32, aspect.height as i32);
            hints.aspect = Some((ratio, ratio));
            hints.set_normal_hints(&connection, window_id)?.check()?;
            Ok(())
        };
        if let Err(error) = apply() {
            eprintln!("Window aspect ratio: {error}");
        }
    })
}

#[cfg(target_os = "android")]
pub fn content_size(_id: window::Id) -> Task<Size> {
    let Ok(app) = crate::deployment::android_assets::activity() else {
        return Task::none();
    };
    let Some(surface) = app.native_window() else {
        return Task::none();
    };
    // Match winit's Android density conversion. These dimensions never include
    // the workbench's application scale, even for queued resize notifications.
    let scale = app.config().density().unwrap_or(160) as f32 / 160.0;
    Task::done(Size::new(
        surface.width() as f32 / scale,
        surface.height() as f32 / scale,
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "android")))]
pub fn content_size(id: window::Id) -> Task<Size> {
    window::size(id)
}

#[cfg(not(target_os = "android"))]
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
        #[ivars = UnboundedSender<crate::Message>]
        struct AImanityMenuActions;

        impl AImanityMenuActions {
            #[unsafe(method(uninstallAImanity:))]
            fn uninstall(&self, _item: &NSMenuItem) {
                let _ = self.ivars().unbounded_send(crate::Message::Uninstall);
            }
            #[unsafe(method(quitAImanity:))]
            fn quit(&self, _item: &NSMenuItem) {
                let _ = self.ivars().unbounded_send(crate::Message::Quit);
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
    let (sender, receiver) = unbounded();
    let actions: Retained<AImanityMenuActions> = unsafe {
        msg_send![
            super(main.alloc::<AImanityMenuActions>().set_ivars(sender)),
            init
        ]
    };
    unsafe {
        quit.setAction(Some(sel!(quitAImanity:)));
        quit.setTarget(Some(&actions));
    }
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
    iced::Task::run(receiver, std::convert::identity)
}

pub fn open_link(value: impl AsRef<str>) -> std::io::Result<()> {
    #[cfg(target_os = "android")]
    return crate::android_os::open("android.intent.action.VIEW", value.as_ref())
        .map_err(std::io::Error::other);
    #[cfg(not(target_os = "android"))]
    open::that_detached(value.as_ref())
}
#[cfg(target_os = "android")]
pub async fn read_clipboard() -> Result<Option<ClipboardContent>, String> {
    tokio::task::spawn_blocking(crate::android_os::clipboard)
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(target_os = "macos")]
pub struct Awake(u32);

#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        kind: core_foundation::string::CFStringRef,
        level: u32,
        name: core_foundation::string::CFStringRef,
        id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

#[cfg(target_os = "macos")]
impl Awake {
    pub fn new() -> Option<Self> {
        use core_foundation::{base::TCFType, string::CFString};
        let kind = CFString::new("PreventUserIdleSystemSleep");
        let name = CFString::new("AImanity workbench and scheduled tasks");
        let mut id = 0;
        let result = unsafe {
            IOPMAssertionCreateWithName(
                kind.as_concrete_TypeRef(),
                255,
                name.as_concrete_TypeRef(),
                &mut id,
            )
        };
        (result == 0).then_some(Self(id))
    }
}

#[cfg(target_os = "macos")]
impl Drop for Awake {
    fn drop(&mut self) {
        unsafe { IOPMAssertionRelease(self.0) };
    }
}
