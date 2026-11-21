use super::{Arg, Env, Object, RawEnv, feed::*};
use core::{
    ffi::{CStr, c_char},
    ptr::null_mut,
};

const SERVICE: &CStr = c"android/accessibilityservice/AccessibilityService";
const OWNER: &CStr = c"Ldev/aiphone/input/PhoneAttention;";
const TIMER: &CStr = c"Ldev/aiphone/input/AttentionTimer;";
const RECT: &CStr = c"Landroid/graphics/Rect;";
const WECHAT: &[u8] = b"com.tencent.mm";

fn active(env: Env, class: Object) -> Object {
    jni!(env, 144, (Object, *const c_char, *const c_char) -> Object,
        class, c"active".as_ptr(), OWNER.as_ptr())
}

fn set_active(env: Env, owner: Object, connected: bool) {
    let class = env.class(c"dev/aiphone/input/PhoneAttention");
    let field = active(env, class);
    let current = jni!(env, 145, (Object, Object) -> Object, class, field);
    if connected || jni!(env, 24, (Object, Object) -> u8, owner, current) != 0 {
        let value = if connected { owner } else { null_mut() };
        jni!(env, 154, (Object, Object, Object) -> (), class, field, value);
    }
}

fn root(env: Env, owner: Object) -> Object {
    env.object(
        owner,
        c"getRootInActiveWindow",
        c"()Landroid/view/accessibility/AccessibilityNodeInfo;",
        &[],
    )
}

fn timer(env: Env, owner: Object, heartbeat: bool) -> Object {
    let timer = env.create(c"dev/aiphone/input/AttentionTimer", c"()V", &[]);
    env.set(timer, c"owner", OWNER, owner);
    env.flag_set(timer, c"heartbeat", heartbeat);
    timer
}

fn persist(env: Env, owner: Object) {
    let file = env.get(owner, c"file", FILE);
    if file.is_null() {
        return;
    }
    let record = env.create(c"org/json/JSONObject", c"()V", &[]);
    let flag = c"(Ljava/lang/String;Z)Lorg/json/JSONObject;";
    let value = c"(Ljava/lang/String;Ljava/lang/Object;)Lorg/json/JSONObject;";
    for (key, field) in [
        (c"connected", c"connected"),
        (c"chat_badge", c"chatBadge"),
        (c"contacts_badge", c"contactsBadge"),
    ] {
        json_put(
            env,
            record,
            key,
            flag,
            int(i32::from(env.flag(owner, field))),
        );
    }
    json_put(
        env,
        record,
        c"checked_at_ms",
        c"(Ljava/lang/String;J)Lorg/json/JSONObject;",
        Arg { long: now(env) },
    );
    json_put(
        env,
        record,
        c"chat_signature",
        value,
        object(env.get(owner, c"signature", STRING)),
    );
    json_put(
        env,
        record,
        c"notifications",
        value,
        object(env.get(owner, c"events", ARRAY)),
    );
    write(env, file, record);
}

fn decode(env: Env, owner: Object, file: Object, events: Object) {
    let Some(value) = saved(env, file) else {
        return;
    };
    let saved = env.object(
        value,
        c"optJSONArray",
        c"(Ljava/lang/String;)Lorg/json/JSONArray;",
        &[object(env.string(c"notifications"))],
    );
    if !saved.is_null() {
        let count = env.integer(saved, c"length");
        for index in (count - 80).max(0)..count {
            let event = env.object(
                saved,
                c"getJSONObject",
                c"(I)Lorg/json/JSONObject;",
                &[int(index)],
            );
            if env.discard_exception() {
                return;
            }
            env.release(env.object(
                events,
                c"put",
                c"(Ljava/lang/Object;)Lorg/json/JSONArray;",
                &[object(event)],
            ));
            env.release(event);
        }
    }
    for (key, field) in [
        (c"chat_badge", c"chatBadge"),
        (c"contacts_badge", c"contactsBadge"),
    ] {
        env.flag_set(
            owner,
            field,
            env.call_bool(
                value,
                c"optBoolean",
                c"(Ljava/lang/String;)Z",
                &[object(env.string(key))],
            ),
        );
    }
    let signature = env.object(
        value,
        c"optString",
        c"(Ljava/lang/String;)Ljava/lang/String;",
        &[object(env.string(c"chat_signature"))],
    );
    env.set(owner, c"signature", STRING, signature);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneAttention_onServiceConnected(
    raw: RawEnv,
    owner: Object,
) {
    let env = Env(raw);
    let previous = env.get(owner, c"handler", HANDLER);
    if !previous.is_null() {
        env.void(
            previous,
            c"removeCallbacksAndMessages",
            c"(Ljava/lang/Object;)V",
            &[object(null_mut())],
        );
    }
    let handler = handler(env);
    let file = file(env, owner, c"attention.json");
    let events = env.create(c"org/json/JSONArray", c"()V", &[]);
    env.set(owner, c"file", FILE, file);
    env.set(owner, c"events", ARRAY, events);
    env.set(owner, c"signature", STRING, env.string(c""));
    env.flag_set(owner, c"capturing", false);
    decode(env, owner, file, events);
    let heartbeat = timer(env, owner, true);
    env.set(owner, c"handler", HANDLER, handler);
    env.set(owner, c"heartbeat", TIMER, heartbeat);
    env.set(owner, c"scan", TIMER, timer(env, owner, false));
    env.flag_set(owner, c"connected", true);
    set_active(env, owner, true);
    env.call_bool(
        handler,
        c"post",
        c"(Ljava/lang/Runnable;)Z",
        &[object(heartbeat)],
    );
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneAttention_onAccessibilityEvent(
    raw: RawEnv,
    owner: Object,
    event: Object,
) {
    let env = Env(raw);
    let package = env.text(event, c"getPackageName");
    if !env.test(package, |name| name == WECHAT) {
        let kind = env.integer(event, c"getEventType");
        // Android reports pane titles and appearances with bits 8 and 16.
        if (matches!(kind, 32 | 2048) && env.integer(event, c"getContentChangeTypes") & 24 != 0)
            || (kind == 32 && {
                let class = env.text(event, c"getClassName");
                env.test(class, |name| name.ends_with(b"Dialog"))
                    || env.call_bool(
                        class,
                        c"equals",
                        c"(Ljava/lang/Object;)Z",
                        &[object(
                            env.string(c"androidx.compose.ui.window.DialogWrapper"),
                        )],
                    )
            })
        {
            emit(
                env,
                owner,
                c"screen:change",
                package,
                c"手机界面变化",
                c"前台应用出现面板或对话框。",
            );
            persist(env, owner);
        }
        return;
    }
    let handler = env.get(owner, c"handler", HANDLER);
    let scan = env.get(owner, c"scan", TIMER);
    env.void(
        handler,
        c"removeCallbacks",
        c"(Ljava/lang/Runnable;)V",
        &[object(scan)],
    );
    env.call_bool(
        handler,
        c"postDelayed",
        c"(Ljava/lang/Runnable;J)Z",
        &[object(scan), Arg { long: 700 }],
    );
}

fn disconnect(env: Env, owner: Object) {
    env.flag_set(owner, c"connected", false);
    set_active(env, owner, false);
    let handler = env.get(owner, c"handler", HANDLER);
    if !handler.is_null() {
        env.void(
            handler,
            c"removeCallbacksAndMessages",
            c"(Ljava/lang/Object;)V",
            &[object(null_mut())],
        );
        persist(env, owner);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneAttention_onUnbind(
    raw: RawEnv,
    owner: Object,
    intent: Object,
) -> u8 {
    let env = Env(raw);
    disconnect(env, owner);
    let class = env.class(SERVICE);
    let method = env.method(class, c"onUnbind", c"(Landroid/content/Intent;)Z");
    let args = [object(intent)];
    jni!(env, 69, (Object, Object, Object, *const Arg) -> u8, owner, class, method, args.as_ptr())
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneAttention_onDestroy(raw: RawEnv, owner: Object) {
    let env = Env(raw);
    disconnect(env, owner);
    let class = env.class(SERVICE);
    let method = env.method(class, c"onDestroy", c"()V");
    jni!(env, 93, (Object, Object, Object, *const Arg) -> (), owner, class, method, core::ptr::null());
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneAttention_onInterrupt(_: RawEnv, _: Object) {}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneAttention_activeRoot(
    raw: RawEnv,
    class: Object,
) -> Object {
    let env = Env(raw);
    let owner = jni!(env, 145, (Object, Object) -> Object, class, active(env, class));
    if owner.is_null() || !env.flag(owner, c"connected") {
        return null_mut();
    }
    root(env, owner)
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_AttentionTimer_run(raw: RawEnv, task: Object) {
    let env = Env(raw);
    let owner = env.get(task, c"owner", OWNER);
    let heartbeat = env.flag(task, c"heartbeat");
    if heartbeat {
        persist(env, owner);
    }
    inspect(env, owner);
    if heartbeat {
        env.call_bool(
            env.get(owner, c"handler", HANDLER),
            c"postDelayed",
            c"(Ljava/lang/Runnable;J)Z",
            &[object(task), Arg { long: 20_000 }],
        );
    }
}

fn metric(env: Env, owner: Object) -> Object {
    let resources = env.object(
        owner,
        c"getResources",
        c"()Landroid/content/res/Resources;",
        &[],
    );
    env.object(
        resources,
        c"getDisplayMetrics",
        c"()Landroid/util/DisplayMetrics;",
        &[],
    )
}

fn emit(env: Env, owner: Object, key: &CStr, package: Object, title: &CStr, body: &CStr) {
    let time = now(env);
    let identity = env.create(c"java/lang/StringBuilder", c"()V", &[]);
    let topic = env.string(key);
    append(env, identity, topic);
    append(env, identity, env.string(c"\n"));
    append(env, identity, long_string(env, time));
    let entry = env.create(c"org/json/JSONObject", c"()V", &[]);
    let value = c"(Ljava/lang/String;Ljava/lang/Object;)Lorg/json/JSONObject;";
    for (key, value_obj) in [
        (
            c"event_id",
            hash(env, env.object(identity, c"toString", STRING_GET, &[])),
        ),
        (c"notification_key", topic),
        (c"package_name", package),
        (
            c"app_name",
            if key.to_bytes().starts_with(b"wechat-") {
                env.string(c"微信")
            } else {
                package
            },
        ),
        (c"title", env.string(title)),
        (c"text", env.string(body)),
    ] {
        json_put(env, entry, key, value, object(value_obj));
    }
    json_put(
        env,
        entry,
        c"posted_at_ms",
        c"(Ljava/lang/String;J)Lorg/json/JSONObject;",
        Arg { long: time },
    );
    let events = env.get(owner, c"events", ARRAY);
    env.release(env.object(
        events,
        c"put",
        c"(Ljava/lang/Object;)Lorg/json/JSONArray;",
        &[object(entry)],
    ));
    if env.integer(events, c"length") > 80 {
        env.release(env.object(events, c"remove", c"(I)Ljava/lang/Object;", &[int(0)]));
    }
}

struct Walk {
    env: Env,
    height: i32,
    rect: Object,
    visible: Object,
    tabs: [Object; 2],
    seen: u16,
    editing: bool,
    password: bool,
}

impl Walk {
    fn visit(&mut self, node: Object, depth: u8) {
        if depth > 35 || self.seen >= 500 {
            return;
        }
        self.seen += 1;
        let env = self.env;
        if env.call_bool(node, c"isVisibleToUser", c"()Z", &[]) {
            if env.call_bool(node, c"isEditable", c"()Z", &[]) {
                self.editing = true;
                self.password |= env.call_bool(node, c"isPassword", c"()Z", &[]);
            } else {
                let label = env.text(node, c"getText");
                env.void(
                    node,
                    c"getBoundsInScreen",
                    c"(Landroid/graphics/Rect;)V",
                    &[object(self.rect)],
                );
                if 4 * env.integer(self.rect, c"centerY") > 3 * self.height {
                    for (name, tab) in [c"微信", c"通讯录"].into_iter().zip(&mut self.tabs) {
                        if env.call_bool(
                            label,
                            c"equals",
                            c"(Ljava/lang/Object;)Z",
                            &[object(env.string(name))],
                        ) {
                            env.release(*tab);
                            *tab = env.create(
                                c"android/graphics/Rect",
                                c"(Landroid/graphics/Rect;)V",
                                &[object(self.rect)],
                            );
                        }
                    }
                }
                if env.test(label, |bytes| {
                    let time = matches!(bytes, [a, b':', b, c] if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit())
                        || matches!(bytes, [a, b, b':', c, d] if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit() && d.is_ascii_digit());
                    !bytes.is_empty() && !time
                }) && !env.call_bool(label, c"contains", c"(Ljava/lang/CharSequence;)Z", &[object(env.string(c"正在输入"))]) {
                    append(env, self.visible, label);
                    append(env, self.visible, env.string(c"\n"));
                }
                env.release(label);
            }
        }
        for index in 0..env.integer(node, c"getChildCount") {
            let child = env.object(
                node,
                c"getChild",
                c"(I)Landroid/view/accessibility/AccessibilityNodeInfo;",
                &[int(index)],
            );
            if !child.is_null() {
                self.visit(child, depth + 1);
                env.void(child, c"recycle", c"()V", &[]);
                env.release(child);
            }
        }
    }
}

fn inspect(env: Env, owner: Object) {
    let node = root(env, owner);
    if node.is_null() {
        return;
    }
    let package = env.text(node, c"getPackageName");
    if !env.test(package, |name| name == WECHAT) {
        env.void(node, c"recycle", c"()V", &[]);
        return;
    }
    let mut walk = Walk {
        env,
        height: env.int_field(metric(env, owner), c"heightPixels"),
        rect: env.create(c"android/graphics/Rect", c"()V", &[]),
        visible: env.create(c"java/lang/StringBuilder", c"()V", &[]),
        tabs: [null_mut(); 2],
        seen: 0,
        editing: false,
        password: false,
    };
    walk.visit(node, 0);
    env.void(node, c"recycle", c"()V", &[]);
    let [chat, contacts] = walk.tabs;
    if !chat.is_null()
        && !contacts.is_null()
        && env.integer(chat, c"centerX") < env.integer(contacts, c"centerX")
        && env
            .integer(chat, c"centerY")
            .abs_diff(env.integer(contacts, c"centerY"))
            < 24
    {
        if !env.flag(owner, c"capturing") {
            env.flag_set(owner, c"capturing", true);
            let callback = env.create(c"dev/aiphone/input/AttentionScreenshot", c"()V", &[]);
            env.set(callback, c"owner", OWNER, owner);
            env.set(callback, c"chat", RECT, chat);
            env.set(callback, c"contacts", RECT, contacts);
            let executor = env.object(
                owner,
                c"getMainExecutor",
                c"()Ljava/util/concurrent/Executor;",
                &[],
            );
            env.void(owner, c"takeScreenshot", c"(ILjava/util/concurrent/Executor;Landroid/accessibilityservice/AccessibilityService$TakeScreenshotCallback;)V",
                &[int(0), object(executor), object(callback)]);
            if env.discard_exception() {
                env.flag_set(owner, c"capturing", false);
            }
        }
        return;
    }
    if !walk.editing || walk.password {
        return;
    }
    let visible = env.object(walk.visible, c"toString", STRING_GET, &[]);
    if jni!(env, 164, (Object) -> i32, visible) == 0
        || env.call_bool(
            visible,
            c"contains",
            c"(Ljava/lang/CharSequence;)Z",
            &[object(env.string(c"找回密码"))],
        )
    {
        return;
    }
    let signature = hash(env, visible);
    let prior = env.get(owner, c"signature", STRING);
    if jni!(env, 164, (Object) -> i32, prior) != 0
        && !env.call_bool(
            prior,
            c"equals",
            c"(Ljava/lang/Object;)Z",
            &[object(signature)],
        )
    {
        emit(
            env,
            owner,
            c"wechat-visible:chat",
            env.string(c"com.tencent.mm"),
            c"微信会话变化",
            c"当前微信可见内容发生变化，请重新观察确认是否有新消息；不保证是对方的新消息。",
        );
    }
    env.set(owner, c"signature", STRING, signature);
    persist(env, owner);
}

fn red_badge(env: Env, bitmap: Object, label: Object, scale: f32) -> bool {
    let center = env.integer(label, c"centerX");
    let left = (center - (32.0 * scale) as i32).max(0);
    let right = (center + (32.0 * scale) as i32).min(env.integer(bitmap, c"getWidth"));
    let label_top = env.int_field(label, c"top");
    let top = (label_top - (34.0 * scale) as i32).max(0);
    let bottom = (label_top + 2).min(env.integer(bitmap, c"getHeight"));
    let (width, height) = (right - left, bottom - top);
    if width <= 0 || height <= 0 {
        return false;
    }
    let len = width * height;
    let array = jni!(env, 179, (i32) -> Object, len);
    env.void(
        bitmap,
        c"getPixels",
        c"([IIIIIII)V",
        &[
            object(array),
            int(0),
            int(width),
            int(left),
            int(top),
            int(width),
            int(height),
        ],
    );
    if env.discard_exception() {
        return false;
    }
    let pixels = jni!(env, 222, (Object, *mut u8) -> *const i32, array, null_mut());
    if pixels.is_null() {
        return false;
    }
    let mut red = 0;
    for &pixel in unsafe { core::slice::from_raw_parts(pixels, len as usize) } {
        let r = (pixel >> 16) & 255;
        let g = (pixel >> 8) & 255;
        let b = pixel & 255;
        if r > 180 && g < 115 && b < 135 && 10 * r > 17 * g && 10 * r > 14 * b {
            red += 1;
        }
    }
    jni!(env, 223, (Object, *const i32, i32) -> (), array, pixels, 2);
    (red as f32) > 12.0 * scale * scale
}

fn inspect_screenshot(env: Env, callback: Object, result: Object, owner: Object, buffer: Object) {
    let current = root(env, owner);
    if current.is_null() {
        return;
    }
    let current_package = env.text(current, c"getPackageName");
    env.void(current, c"recycle", c"()V", &[]);
    if !env.test(current_package, |name| name == WECHAT) {
        return;
    }
    let color = env.object(
        result,
        c"getColorSpace",
        c"()Landroid/graphics/ColorSpace;",
        &[],
    );
    let raw = env.static_object(c"android/graphics/Bitmap", c"wrapHardwareBuffer", c"(Landroid/hardware/HardwareBuffer;Landroid/graphics/ColorSpace;)Landroid/graphics/Bitmap;",
        &[object(buffer), object(color)]);
    if raw.is_null() {
        return;
    }
    let class = env.class(c"android/graphics/Bitmap$Config");
    let field = jni!(env, 144, (Object, *const c_char, *const c_char) -> Object, class, c"ARGB_8888".as_ptr(), c"Landroid/graphics/Bitmap$Config;".as_ptr());
    let config = jni!(env, 145, (Object, Object) -> Object, class, field);
    let bitmap = env.object(
        raw,
        c"copy",
        c"(Landroid/graphics/Bitmap$Config;Z)Landroid/graphics/Bitmap;",
        &[object(config), int(0)],
    );
    env.void(raw, c"recycle", c"()V", &[]);
    if bitmap.is_null() {
        return;
    }
    let metrics = metric(env, owner);
    let field = env.field(metrics, c"density", c"F");
    let scale = jni!(env, 102, (Object, Object) -> f32, metrics, field);
    for (tab, state, key, title, body) in [
        (
            c"chat",
            c"chatBadge",
            c"wechat-badge:0",
            c"微信未读",
            c"底部微信出现未读标记，请打开核对。",
        ),
        (
            c"contacts",
            c"contactsBadge",
            c"wechat-badge:1",
            c"通讯录未读",
            c"底部通讯录出现未读标记，请打开核对。",
        ),
    ] {
        let unread = red_badge(env, bitmap, env.get(callback, tab, RECT), scale);
        if unread && !env.flag(owner, state) {
            emit(env, owner, key, env.string(c"com.tencent.mm"), title, body);
        }
        env.flag_set(owner, state, unread);
    }
    env.void(bitmap, c"recycle", c"()V", &[]);
    persist(env, owner);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_AttentionScreenshot_onSuccess(
    raw: RawEnv,
    callback: Object,
    result: Object,
) {
    let env = Env(raw);
    let owner = env.get(callback, c"owner", OWNER);
    let buffer = env.object(
        result,
        c"getHardwareBuffer",
        c"()Landroid/hardware/HardwareBuffer;",
        &[],
    );
    inspect_screenshot(env, callback, result, owner, buffer);
    env.discard_exception();
    env.void(buffer, c"close", c"()V", &[]);
    env.flag_set(owner, c"capturing", false);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_AttentionScreenshot_onFailure(
    raw: RawEnv,
    callback: Object,
    _: i32,
) {
    let env = Env(raw);
    env.flag_set(env.get(callback, c"owner", OWNER), c"capturing", false);
}
