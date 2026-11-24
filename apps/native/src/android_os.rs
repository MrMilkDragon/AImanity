use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JIntArray, JObject, JObjectArray, JString, JValue},
    sys::jint,
};
use std::{fs::File, os::fd::FromRawFd, sync::Mutex};
use tokio::sync::oneshot;

pub(crate) type Result<T> = std::result::Result<T, String>;
pub(crate) fn with<T>(
    f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>,
) -> Result<T> {
    let app = crate::deployment::android_assets::activity().map_err(|e| e.to_string())?;
    with_activity(&app, f)
}

fn with_activity<T>(
    app: &android_activity::AndroidApp,
    f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>,
) -> Result<T> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let res = env.with_local_frame(64, |env| {
        // AndroidApp owns the global Activity reference. Only wrap our local copy.
        let raw = unsafe {
            let jni = env.get_native_interface();
            ((**jni).NewLocalRef.unwrap())(jni, app.activity_as_ptr().cast())
        };
        if raw.is_null() {
            return Err(jni::errors::Error::NullPtr("NativeActivity"));
        }
        let activity = unsafe { JObject::from_raw(raw) };
        f(env, &activity)
    });
    if res.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    res.map_err(|e| e.to_string())
}

pub(crate) fn string(env: &mut JNIEnv, obj: JObject) -> jni::errors::Result<String> {
    Ok(env.get_string(&JString::from(obj))?.into())
}
fn uri<'a>(env: &mut JNIEnv<'a>, value: &str) -> jni::errors::Result<JObject<'a>> {
    let value = env.new_string(value)?;
    env.call_static_method(
        "android/net/Uri",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[(&value).into()],
    )?
    .l()
}
pub(crate) fn intent<'a>(env: &mut JNIEnv<'a>, action: &str) -> jni::errors::Result<JObject<'a>> {
    let name = env.new_string(action)?;
    env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;)V",
        &[(&name).into()],
    )
}
pub(crate) fn open(action: &str, value: &str) -> Result<()> {
    with(|e, a| {
        let request = intent(e, action)?;
        let data = uri(e, value)?;
        e.call_method(
            &request,
            "setData",
            "(Landroid/net/Uri;)Landroid/content/Intent;",
            &[(&data).into()],
        )?;
        e.call_method(
            a,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[(&request).into()],
        )?;
        Ok(())
    })
}
pub(crate) fn uri_file(value: &str, mode: &str) -> Result<File> {
    let fd = with(|e, a| {
        let resolver = e
            .call_method(
                a,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;
        let data = uri(e, value)?;
        let mode = e.new_string(mode)?;
        let descriptor = e
            .call_method(
                &resolver,
                "openFileDescriptor",
                "(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;",
                &[(&data).into(), (&mode).into()],
            )?
            .l()?;
        e.call_method(&descriptor, "detachFd", "()I", &[])?.i()
    })?;
    // detachFd transfers ownership of the open file descriptor to this process.
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub(crate) fn create_child(tree: &str, name: &str) -> Result<String> {
    with(|e, a| {
        const DOCS: &str = "android/provider/DocumentsContract";
        let root = uri(e, tree)?;
        let id = e
            .call_static_method(
                DOCS,
                "getTreeDocumentId",
                "(Landroid/net/Uri;)Ljava/lang/String;",
                &[(&root).into()],
            )?
            .l()?;
        let parent = e
            .call_static_method(
                DOCS,
                "buildDocumentUriUsingTree",
                "(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;",
                &[(&root).into(), (&id).into()],
            )?
            .l()?;
        let mime = e.new_string("application/octet-stream")?;
        let name = e.new_string(name)?;
        let r = e
            .call_method(
                a,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;
        let child = e.call_static_method(DOCS,"createDocument","(Landroid/content/ContentResolver;Landroid/net/Uri;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",&[(&r).into(),(&parent).into(),(&mime).into(),(&name).into()])?.l()?;
        let text = e
            .call_method(child, "toString", "()Ljava/lang/String;", &[])?
            .l()?;
        string(e, text)
    })
}
pub(crate) fn file_name(value: &str) -> Result<String> {
    with(|e, a| {
        let data = uri(e, value)?;
        let resolver = e
            .call_method(
                a,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;
        let null = JObject::null();
        let projection =
            e.new_object_array(1, "java/lang/String", e.new_string("_display_name")?)?;
        let cursor = e.call_method(resolver,"query","(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",&[(&data).into(),(&projection).into(),(&null).into(),(&null).into(),(&null).into()])?.l()?;
        let exists = e.call_method(&cursor, "moveToFirst", "()Z", &[])?.z()?;
        let name = if exists {
            let text = e
                .call_method(&cursor, "getString", "(I)Ljava/lang/String;", &[0.into()])?
                .l()?;
            string(e, text)?
        } else {
            String::new()
        };
        e.call_method(cursor, "close", "()V", &[])?;
        Ok(name)
    })
}

static PICK: Mutex<Option<oneshot::Sender<Result<Vec<String>>>>> = Mutex::new(None);
pub(crate) async fn choose(
    action: &str,
    mime: &str,
    title: Option<&str>,
    multiple: bool,
) -> Result<Vec<String>> {
    let (tx, rx) = oneshot::channel();
    {
        let mut active = PICK.lock().map_err(|e| e.to_string())?;
        if active.is_some() {
            return Err("Another document picker is open".into());
        }
        *active = Some(tx);
    }
    let launch = with(|e, a| {
        let request = intent(e, action)?;
        if !action.ends_with("TREE") {
            let category = e.new_string("android.intent.category.OPENABLE")?;
            e.call_method(
                &request,
                "addCategory",
                "(Ljava/lang/String;)Landroid/content/Intent;",
                &[(&category).into()],
            )?;
            let mime = e.new_string(mime)?;
            e.call_method(
                &request,
                "setType",
                "(Ljava/lang/String;)Landroid/content/Intent;",
                &[(&mime).into()],
            )?;
        }
        if multiple {
            let key = e.new_string("android.intent.extra.ALLOW_MULTIPLE")?;
            e.call_method(
                &request,
                "putExtra",
                "(Ljava/lang/String;Z)Landroid/content/Intent;",
                &[(&key).into(), JValue::Bool(1)],
            )?;
        }
        if let Some(title) = title {
            let key = e.new_string("android.intent.extra.TITLE")?;
            let title = e.new_string(title)?;
            e.call_method(
                &request,
                "putExtra",
                "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
                &[(&key).into(), (&title).into()],
            )?;
        }
        e.call_method(
            a,
            "startActivityForResult",
            "(Landroid/content/Intent;I)V",
            &[(&request).into(), 1401.into()],
        )?;
        Ok(())
    });
    if let Err(err) = launch {
        PICK.lock().unwrap().take();
        return Err(err);
    }
    rx.await.map_err(|e| e.to_string())?
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_aimanity_mobile_WorkbenchActivity_onActivityResult(
    mut e: JNIEnv,
    _: JObject,
    request: jint,
    code: jint,
    data: JObject,
) {
    if request != 1401 {
        return;
    }
    let Some(tx) = PICK.lock().unwrap().take() else {
        return;
    };
    let result = (|| -> jni::errors::Result<Vec<String>> {
        let mut entries = Vec::new();
        if code != -1 || data.is_null() {
            return Ok(entries);
        }
        let clip = e
            .call_method(&data, "getClipData", "()Landroid/content/ClipData;", &[])?
            .l()?;
        if clip.is_null() {
            let item = e
                .call_method(&data, "getData", "()Landroid/net/Uri;", &[])?
                .l()?;
            let value = e
                .call_method(item, "toString", "()Ljava/lang/String;", &[])?
                .l()?;
            entries.push(string(&mut e, value)?);
        } else {
            let count = e.call_method(&clip, "getItemCount", "()I", &[])?.i()?;
            for idx in 0..count {
                entries.push(e.with_local_frame(8, |e| -> jni::errors::Result<String> {
                    let item = e
                        .call_method(
                            &clip,
                            "getItemAt",
                            "(I)Landroid/content/ClipData$Item;",
                            &[idx.into()],
                        )?
                        .l()?;
                    let uri = e
                        .call_method(item, "getUri", "()Landroid/net/Uri;", &[])?
                        .l()?;
                    let v = e
                        .call_method(uri, "toString", "()Ljava/lang/String;", &[])?
                        .l()?;
                    string(e, v)
                })?);
            }
        }
        Ok(entries)
    })();
    let _ = tx.send(result.map_err(|e| e.to_string()));
}

static MIC: Mutex<Vec<oneshot::Sender<Result<()>>>> = Mutex::new(Vec::new());

fn microphone_result(result: Result<()>) {
    for reply in MIC.lock().unwrap().drain(..) {
        let _ = reply.send(result.clone());
    }
}

pub(crate) async fn microphone() -> Result<()> {
    let app = crate::deployment::android_assets::activity().map_err(|e| e.to_string())?;
    let (tx, rx) = oneshot::channel();
    let first = {
        let mut pending = MIC.lock().unwrap();
        pending.push(tx);
        pending.len() == 1
    };
    if first {
        app.run_on_java_main_thread(Box::new(|| {
            let request = with(|e, a| {
                let p = e.new_string("android.permission.RECORD_AUDIO")?;
                if e.call_method(a, "checkSelfPermission", "(Ljava/lang/String;)I", &[(&p).into()])?.i()? == 0 {
                    return Ok(true);
                }
                let permissions = e.new_object_array(1, "java/lang/String", &p)?;
                e.call_method(a, "requestPermissions", "([Ljava/lang/String;I)V", &[(&permissions).into(), 1402.into()])?;
                Ok(false)
            });
            match request {
                Ok(false) => {}
                Ok(true) => microphone_result(Ok(())),
                Err(e) => microphone_result(Err(e)),
            }
        }));
    }
    rx.await.map_err(|e| e.to_string())?
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_aimanity_mobile_WorkbenchActivity_onRequestPermissionsResult(
    e: JNIEnv,
    _: JObject,
    request: jint,
    _: JObjectArray,
    results: JIntArray,
) {
    if request == 1402 {
        let mut first = [-1];
        let allowed = e.get_array_length(&results).is_ok_and(|len| len > 0)
            && e.get_int_array_region(&results, 0, &mut first).is_ok()
            && first[0] == 0;
        microphone_result(if allowed { Ok(()) } else { Err("Microphone permission was declined".into()) });
    }
}

pub(crate) fn cache() -> Result<std::path::PathBuf> {
    with(|e, a| {
        let dir = e
            .call_method(a, "getCacheDir", "()Ljava/io/File;", &[])?
            .l()?;
        let path = e
            .call_method(dir, "getAbsolutePath", "()Ljava/lang/String;", &[])?
            .l()?;
        Ok(std::path::PathBuf::from(string(e, path)?))
    })
}
pub(crate) fn clipboard() -> Result<Option<crate::platform::ClipboardContent>> {
    let (text, uri) = with(|e, a| {
        let key = e.new_string("clipboard")?;
        let mgr = e
            .call_method(
                a,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&key).into()],
            )?
            .l()?;
        let clip = e
            .call_method(mgr, "getPrimaryClip", "()Landroid/content/ClipData;", &[])?
            .l()?;
        if clip.is_null() {
            return Ok((None, None));
        }
        let item = e
            .call_method(
                clip,
                "getItemAt",
                "(I)Landroid/content/ClipData$Item;",
                &[0.into()],
            )?
            .l()?;
        let text = e
            .call_method(&item, "getText", "()Ljava/lang/CharSequence;", &[])?
            .l()?;
        if !text.is_null() {
            let v = e
                .call_method(text, "toString", "()Ljava/lang/String;", &[])?
                .l()?;
            return Ok((Some(string(e, v)?), None));
        }
        let data = e
            .call_method(&item, "getUri", "()Landroid/net/Uri;", &[])?
            .l()?;
        if data.is_null() {
            return Ok((None, None));
        }
        let v = e
            .call_method(data, "toString", "()Ljava/lang/String;", &[])?
            .l()?;
        Ok((None, Some(string(e, v)?)))
    })?;
    if let Some(text) = text {
        return Ok(Some(crate::platform::ClipboardContent::Text(text)));
    }
    if let Some(uri) = uri {
        let reader = image::ImageReader::new(std::io::BufReader::new(uri_file(&uri, "r")?))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        if reader.format().is_none() {
            return Ok(None);
        }
        let image = reader.decode().map_err(|e| e.to_string())?;
        let mut png = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        return Ok(Some(crate::platform::ClipboardContent::Png(
            png.into_inner(),
        )));
    }
    Ok(None)
}

fn app_object<'a>(e: &mut JNIEnv<'a>, a: &JObject, name: &str) -> jni::errors::Result<JObject<'a>> {
    let loader = e
        .call_method(a, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = e.new_string(name)?;
    let class = e
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )?
        .l()?;
    e.new_object(jni::objects::JClass::from(class), "()V", &[])
}
type SpeechEngine = std::sync::Arc<tokio::sync::OnceCell<GlobalRef>>;
static ENGINE: Mutex<Option<SpeechEngine>> = Mutex::new(None);
static INIT: Mutex<Option<oneshot::Sender<jint>>> = Mutex::new(None);
type Utterance = (String, oneshot::Sender<bool>);
static PHRASE: Mutex<Option<Utterance>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_aimanity_mobile_SpeechInit_onInit(
    _: JNIEnv,
    _: JObject,
    status: jint,
) {
    if let Some(tx) = INIT.lock().unwrap().take() {
        let _ = tx.send(status);
    }
}
fn tts_completed(e: &mut JNIEnv, id: JString, ok: bool) {
    let Ok(id) = e.get_string(&id) else {
        return;
    };
    let id: String = id.into();
    if let Some((_, tx)) = PHRASE
        .lock()
        .unwrap()
        .take_if(|(current, _)| *current == id)
    {
        let _ = tx.send(ok);
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_aimanity_mobile_SpeechProgress_onStart(
    _: JNIEnv,
    _: JObject,
    _: JString,
) {
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_aimanity_mobile_SpeechProgress_onDone(
    mut e: JNIEnv,
    _: JObject,
    id: JString,
) {
    tts_completed(&mut e, id, true)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_aimanity_mobile_SpeechProgress_onError(
    mut e: JNIEnv,
    _: JObject,
    id: JString,
) {
    tts_completed(&mut e, id, false)
}

pub(crate) async fn speak(
    text: String,
    language: String,
    mut cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let session = ENGINE
        .lock()
        .unwrap()
        .get_or_insert_with(Default::default)
        .clone();
    let engine = session
        .get_or_try_init(|| async {
            let (tx, rx) = oneshot::channel();
            *INIT.lock().unwrap() = Some(tx);
            let engine = with(|e, a| {
                let listener = app_object(e, a, "dev.aimanity.mobile.SpeechInit")?;
                let ctx = e
                    .call_method(
                        a,
                        "getApplicationContext",
                        "()Landroid/content/Context;",
                        &[],
                    )?
                    .l()?;
                let tts = e.new_object(
                    "android/speech/tts/TextToSpeech",
                    "(Landroid/content/Context;Landroid/speech/tts/TextToSpeech$OnInitListener;)V",
                    &[(&ctx).into(), (&listener).into()],
                )?;
                let progress = app_object(e, a, "dev.aimanity.mobile.SpeechProgress")?;
                e.call_method(
                    &tts,
                    "setOnUtteranceProgressListener",
                    "(Landroid/speech/tts/UtteranceProgressListener;)I",
                    &[(&progress).into()],
                )?;
                e.new_global_ref(tts)
            })?;
            if rx.await.map_err(|e| e.to_string())? != 0 {
                return Err("Android speech engine unavailable".to_string());
            }
            Ok(engine)
        })
        .await?;
    let id = uuid::Uuid::now_v7().to_string();
    let (tx, rx) = oneshot::channel();
    *PHRASE.lock().unwrap() = Some((id.clone(), tx));
    let status = with(|e, _| {
        let lang = e.new_string(language)?;
        let locale = e
            .call_static_method(
                "java/util/Locale",
                "forLanguageTag",
                "(Ljava/lang/String;)Ljava/util/Locale;",
                &[(&lang).into()],
            )?
            .l()?;
        e.call_method(
            engine,
            "setLanguage",
            "(Ljava/util/Locale;)I",
            &[(&locale).into()],
        )?;
        let text = e.new_string(text)?;
        let id = e.new_string(&id)?;
        e.call_method(
            engine,
            "speak",
            "(Ljava/lang/CharSequence;ILandroid/os/Bundle;Ljava/lang/String;)I",
            &[
                (&text).into(),
                0.into(),
                (&JObject::null()).into(),
                (&id).into(),
            ],
        )?
        .i()
    })?;
    if status != 0 {
        PHRASE.lock().unwrap().take();
        return Err("Android speech failed".into());
    }
    tokio::select! {
        result=rx=>if result.unwrap_or(false){Ok(())}else{Err("Android speech failed".into())},
        _=&mut cancel=>{ if PHRASE.lock().unwrap().take_if(|(current,_)| *current==id).is_some(){with(|e,_|{e.call_method(engine,"stop","()I",&[])?;Ok(())})?;} Ok(())}
    }
}

pub(crate) fn detach(app: &android_activity::AndroidApp) {
    if !crate::deployment::android_assets::detach(app) {
        return;
    }
    PICK.lock().unwrap().take();
    MIC.lock().unwrap().clear();
    INIT.lock().unwrap().take();
    PHRASE.lock().unwrap().take();
    if let Some(session) = ENGINE.lock().unwrap().take()
        && let Some(engine) = session.get()
    {
        let _ = with_activity(app, |e, _| {
            e.call_method(engine, "shutdown", "()V", &[])?;
            Ok(())
        });
    }
}
