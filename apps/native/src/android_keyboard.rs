//! Android delivers composition through an InputConnection on the UI thread.
//! Iced owns the text and selection; the system view only mediates the keyboard.
use android_activity::AndroidApp;
use iced::{Event, advanced::input_method, keyboard};
use jni::{
    JNIEnv,
    objects::{GlobalRef, JClass, JObject},
    sys::{jboolean, jint, jobject},
};
use std::sync::{
    Mutex,
    atomic::{AtomicI32, Ordering},
};

static ACTIVITY: Mutex<Option<AndroidApp>> = Mutex::new(None);
static VIEW: Mutex<Option<GlobalRef>> = Mutex::new(None);
static GENERATION: AtomicI32 = AtomicI32::new(0);
static INPUT_TYPE: AtomicI32 = AtomicI32::new(1);

pub fn attach(app: AndroidApp) {
    *ACTIVITY.lock().unwrap() = Some(app);
}
pub fn detach() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    ACTIVITY.lock().unwrap().take();
    VIEW.lock().unwrap().take();
}
fn app_class<'a>(
    env: &mut JNIEnv<'a>,
    activity: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'a>> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(name)?;
    let class = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )?
        .l()?;
    Ok(class.into())
}
pub fn show(purpose: Option<input_method::Purpose>) {
    let Some(app) = ACTIVITY.lock().unwrap().clone() else {
        return;
    };
    GENERATION.fetch_add(1, Ordering::Relaxed);
    INPUT_TYPE.store(
        if purpose == Some(input_method::Purpose::Secure) {
            0x81
        } else {
            1
        },
        Ordering::Relaxed,
    );
    app.run_on_java_main_thread(Box::new(move || {
        let result = crate::android_os::with(|env, activity| {
            env.with_local_frame(24, |env| {
                let service = env.new_string("input_method")?;
                let imm = env
                    .call_method(
                        activity,
                        "getSystemService",
                        "(Ljava/lang/String;)Ljava/lang/Object;",
                        &[(&service).into()],
                    )?
                    .l()?;
                let mut slot = VIEW.lock().unwrap();
                if purpose.is_some() {
                    if slot.is_none() {
                        let class = app_class(env, activity, "dev.aimanity.mobile.KeyboardView")?;
                        let view = env.new_object(
                            class,
                            "(Landroid/content/Context;)V",
                            &[activity.into()],
                        )?;
                        env.call_method(&view, "setFocusableInTouchMode", "(Z)V", &[true.into()])?;
                        // The 1px editor must not steal touch gestures or present an empty
                        // accessibility element on top of the native surface.
                        env.call_method(
                            &view,
                            "setImportantForAccessibility",
                            "(I)V",
                            &[2.into()],
                        )?;
                        let layout = env.new_object(
                            "android/view/ViewGroup$LayoutParams",
                            "(II)V",
                            &[1.into(), 1.into()],
                        )?;
                        env.call_method(
                            activity,
                            "addContentView",
                            "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V",
                            &[(&view).into(), (&layout).into()],
                        )?;
                        *slot = Some(env.new_global_ref(view)?);
                    }
                    let view = slot.as_ref().unwrap().as_obj();
                    if env.call_method(view, "hasFocus", "()Z", &[])?.z()? {
                        env.call_method(
                            &imm,
                            "restartInput",
                            "(Landroid/view/View;)V",
                            &[view.into()],
                        )?;
                    } else {
                        env.call_method(view, "requestFocus", "()Z", &[])?;
                    }
                    env.call_method(
                        &imm,
                        "showSoftInput",
                        "(Landroid/view/View;I)Z",
                        &[view.into(), 1.into()],
                    )?;
                } else if let Some(view) = slot.as_ref() {
                    let token = env
                        .call_method(view, "getWindowToken", "()Landroid/os/IBinder;", &[])?
                        .l()?;
                    env.call_method(
                        &imm,
                        "hideSoftInputFromWindow",
                        "(Landroid/os/IBinder;I)Z",
                        &[(&token).into(), 0.into()],
                    )?;
                    env.call_method(view, "clearFocus", "()V", &[])?;
                }
                Ok(())
            })
        });
        if let Err(error) = result {
            eprintln!("Android input method: {error}");
        }
    }));
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardView_onCheckIsTextEditor(
    _: JNIEnv,
    _: JObject,
) -> jboolean {
    1
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardView_onCreateInputConnection(
    mut env: JNIEnv,
    view: JObject,
    info: JObject,
) -> jobject {
    let result = (|| -> jni::errors::Result<jobject> {
        env.set_field(
            &info,
            "inputType",
            "I",
            INPUT_TYPE.load(Ordering::Relaxed).into(),
        )?;
        // DONE and NO_EXTRACT_UI keep landscape keyboards from replacing the workbench.
        env.set_field(
            &info,
            "imeOptions",
            "I",
            (0x1000_0000 | 0x0200_0000 | 6).into(),
        )?;
        env.set_field(&info, "initialSelStart", "I", 0.into())?;
        env.set_field(&info, "initialSelEnd", "I", 0.into())?;
        let connection = env.new_object(
            "dev/aimanity/mobile/KeyboardConnection",
            "(Landroid/view/View;Z)V",
            &[(&view).into(), true.into()],
        )?;
        env.set_field(
            &connection,
            "generation",
            "I",
            GENERATION.load(Ordering::Relaxed).into(),
        )?;
        iced_winit::android_input(Event::InputMethod(input_method::Event::Opened));
        Ok(connection.into_raw())
    })();
    result.unwrap_or(std::ptr::null_mut())
}
fn named(name: keyboard::key::Named) {
    let key = keyboard::Key::Named(name);
    iced_winit::android_input(Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    }));
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardView_onKeyPreIme(
    mut env: JNIEnv,
    _: JObject,
    code: jint,
    event: JObject,
) -> jboolean {
    if code == 4
        && env
            .call_method(event, "getAction", "()I", &[])
            .and_then(|r| r.i())
            .ok()
            == Some(1)
    {
        iced_winit::android_input(Event::InputMethod(input_method::Event::Closed));
    }
    0
}
fn current(env: &mut JNIEnv, connection: &JObject) -> bool {
    env.get_field(connection, "generation", "I")
        .and_then(|v| v.i())
        .ok()
        == Some(GENERATION.load(Ordering::Relaxed))
}
fn clear_composition() {
    iced_winit::android_input(Event::InputMethod(input_method::Event::Preedit(
        String::new(),
        None,
    )));
}
fn edit(
    env: &mut JNIEnv,
    connection: &JObject,
    sequence: JObject,
    compose: bool,
) -> jni::errors::Result<()> {
    if !current(env, connection) {
        return Ok(());
    }
    let value = env
        .call_method(sequence, "toString", "()Ljava/lang/String;", &[])?
        .l()?;
    let text = crate::android_os::string(env, env.new_local_ref(&value)?)?;
    if compose {
        env.set_field(connection, "preedit", "Ljava/lang/String;", (&value).into())?;
        let end = text.len();
        iced_winit::android_input(Event::InputMethod(input_method::Event::Preedit(
            text,
            Some(end..end),
        )));
    } else {
        env.set_field(
            connection,
            "preedit",
            "Ljava/lang/String;",
            (&JObject::null()).into(),
        )?;
        clear_composition();
        if !text.is_empty() {
            iced_winit::android_input(Event::InputMethod(input_method::Event::Commit(text)));
        }
    }
    Ok(())
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_setComposingText(
    mut env: JNIEnv,
    connection: JObject,
    text: JObject,
    _: jint,
) -> jboolean {
    edit(&mut env, &connection, text, true).is_ok().into()
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_commitText(
    mut env: JNIEnv,
    connection: JObject,
    text: JObject,
    _: jint,
) -> jboolean {
    edit(&mut env, &connection, text, false).is_ok().into()
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_finishComposingText(
    mut env: JNIEnv,
    connection: JObject,
) -> jboolean {
    let result = (|| -> jni::errors::Result<()> {
        let preedit = env
            .get_field(&connection, "preedit", "Ljava/lang/String;")?
            .l()?;
        if !preedit.is_null() {
            edit(&mut env, &connection, preedit, false)?;
        }
        Ok(())
    })();
    result.is_ok().into()
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_deleteSurroundingText(
    mut env: JNIEnv,
    connection: JObject,
    before: jint,
    after: jint,
) -> jboolean {
    if !current(&mut env, &connection) {
        return 0;
    }
    for _ in 0..before.clamp(0, 2048) {
        named(keyboard::key::Named::Backspace);
    }
    for _ in 0..after.clamp(0, 2048) {
        named(keyboard::key::Named::Delete);
    }
    1
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_deleteSurroundingTextInCodePoints(
    env: JNIEnv,
    connection: JObject,
    before: jint,
    after: jint,
) -> jboolean {
    Java_dev_aimanity_mobile_KeyboardConnection_deleteSurroundingText(
        env, connection, before, after,
    )
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_performEditorAction(
    mut env: JNIEnv,
    connection: JObject,
    _: jint,
) -> jboolean {
    if !current(&mut env, &connection) {
        return 0;
    }
    named(keyboard::key::Named::Enter);
    1
}
#[unsafe(no_mangle)]
extern "system" fn Java_dev_aimanity_mobile_KeyboardConnection_sendKeyEvent(
    mut env: JNIEnv,
    connection: JObject,
    event: JObject,
) -> jboolean {
    if !current(&mut env, &connection) {
        return 0;
    }
    let result = (|| -> jni::errors::Result<bool> {
        if env.call_method(&event, "getAction", "()I", &[])?.i()? != 0 {
            return Ok(true);
        }
        let code = env.call_method(&event, "getKeyCode", "()I", &[])?.i()?;
        use keyboard::key::Named;
        match code {
            19 => named(Named::ArrowUp),
            20 => named(Named::ArrowDown),
            21 => named(Named::ArrowLeft),
            22 => named(Named::ArrowRight),
            61 => named(Named::Tab),
            66 => named(Named::Enter),
            67 => named(Named::Backspace),
            112 => named(Named::Delete),
            _ => {
                let scalar = env.call_method(event, "getUnicodeChar", "()I", &[])?.i()?;
                let Some(ch) = char::from_u32(scalar as u32).filter(|c| !c.is_control()) else {
                    return Ok(false);
                };
                iced_winit::android_input(Event::InputMethod(input_method::Event::Commit(
                    ch.to_string(),
                )));
            }
        }
        Ok(true)
    })();
    result.unwrap_or(false).into()
}
