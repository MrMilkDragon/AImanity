use super::{Arg, Env, Object, RawEnv, feed::*};
use core::{ffi::CStr, ptr::null_mut};

const SERVICE: &CStr = c"android/service/notification/NotificationListenerService";
const QUEUE: &CStr = c"Ljava/util/ArrayDeque;";
const SEEN: &CStr = c"Ljava/util/HashSet;";
const LONG_VALUE: &CStr = c"(Ljava/lang/String;J)Lorg/json/JSONObject;";
const OBJECT_VALUE: &CStr = c"(Ljava/lang/String;Ljava/lang/Object;)Lorg/json/JSONObject;";

fn superclass(env: Env, owner: Object, name: &CStr) {
    let class = env.class(SERVICE);
    let method = env.method(class, name, c"()V");
    jni!(env, 93, (Object, Object, Object, *const Arg) -> (), owner, class, method, core::ptr::null());
}

fn property(env: Env, node: Object, name: &CStr) -> Object {
    env.object(node, name, STRING_GET, &[])
}

fn bounded(env: Env, value: Object, limit: i32) -> Object {
    if value.is_null() {
        return env.string(c"");
    }
    let text = property(env, value, c"toString");
    if jni!(env, 164, (Object) -> i32, text) > limit {
        env.object(
            text,
            c"substring",
            c"(II)Ljava/lang/String;",
            &[int(0), int(limit)],
        )
    } else {
        text
    }
}

fn event_id(env: Env, event: Object) -> Object {
    env.object(
        event,
        c"optString",
        c"(Ljava/lang/String;)Ljava/lang/String;",
        &[object(env.string(c"event_id"))],
    )
}

fn body_size(env: Env, event: Object) -> i32 {
    let bytes = env.bytes(property(env, event, c"toString"));
    jni!(env, 171, (Object) -> i32, bytes)
}

fn enqueue(env: Env, owner: Object, event: Object) {
    let events = env.get(owner, c"events", QUEUE);
    let seen = env.get(owner, c"seen", SEEN);
    let field = env.field(owner, c"bufferedBytes", c"I");
    let mut bytes = jni!(env, 100, (Object, Object) -> i32, owner, field) + body_size(env, event);
    env.call_bool(events, c"add", c"(Ljava/lang/Object;)Z", &[object(event)]);
    env.call_bool(
        seen,
        c"add",
        c"(Ljava/lang/Object;)Z",
        &[object(event_id(env, event))],
    );
    while env.integer(events, c"size") > 160 || bytes > 1_200_000 {
        let removed = env.object(events, c"removeFirst", c"()Ljava/lang/Object;", &[]);
        env.call_bool(
            seen,
            c"remove",
            c"(Ljava/lang/Object;)Z",
            &[object(event_id(env, removed))],
        );
        bytes -= body_size(env, removed);
        env.release(removed);
    }
    jni!(env, 109, (Object, Object, i32) -> (), owner, field, bytes);
}

fn persist(env: Env, owner: Object) {
    let file = env.get(owner, c"file", FILE);
    if file.is_null() {
        return;
    }
    let record = env.create(c"org/json/JSONObject", c"()V", &[]);
    let events = env.get(owner, c"events", QUEUE);
    let array = env.create(
        c"org/json/JSONArray",
        c"(Ljava/util/Collection;)V",
        &[object(events)],
    );
    json_put(
        env,
        record,
        c"connected",
        c"(Ljava/lang/String;Z)Lorg/json/JSONObject;",
        int(i32::from(env.flag(owner, c"connected"))),
    );
    json_put(
        env,
        record,
        c"checked_at_ms",
        LONG_VALUE,
        Arg { long: now(env) },
    );
    json_put(env, record, c"notifications", OBJECT_VALUE, object(array));
    write(env, file, record);
}

fn notification_text(env: Env, extras: Object, name: &CStr, limit: i32) -> Object {
    let value = env.object(
        extras,
        c"getCharSequence",
        c"(Ljava/lang/String;)Ljava/lang/CharSequence;",
        &[object(env.string(name))],
    );
    bounded(env, value, limit)
}

fn app_label(env: Env, owner: Object, package: Object) -> Object {
    let manager = env.object(
        owner,
        c"getPackageManager",
        c"()Landroid/content/pm/PackageManager;",
        &[],
    );
    let app = env.object(
        manager,
        c"getApplicationInfo",
        c"(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;",
        &[object(package), int(0)],
    );
    if env.discard_exception() {
        return package;
    }
    let label = env.object(
        manager,
        c"getApplicationLabel",
        c"(Landroid/content/pm/ApplicationInfo;)Ljava/lang/CharSequence;",
        &[object(app)],
    );
    bounded(env, label, 200)
}

fn remember(env: Env, owner: Object, sbn: Object) {
    let notification = env.object(
        sbn,
        c"getNotification",
        c"()Landroid/app/Notification;",
        &[],
    );
    if env.int_field(notification, c"flags") & (2 | 512) != 0 {
        return;
    }
    let extras = env.get(notification, c"extras", c"Landroid/os/Bundle;");
    if extras.is_null() {
        return;
    }
    let title = notification_text(env, extras, c"android.title", 500);
    let mut body = notification_text(env, extras, c"android.bigText", 4000);
    if jni!(env, 164, (Object) -> i32, body) == 0 {
        body = notification_text(env, extras, c"android.text", 4000);
    }
    if jni!(env, 164, (Object) -> i32, body) == 0 && jni!(env, 164, (Object) -> i32, title) == 0 {
        return;
    }
    let key = property(env, sbn, c"getKey");
    let package = property(env, sbn, c"getPackageName");
    let post = env.instance_method(sbn, c"getPostTime", c"()J");
    let posted = jni!(env, 54, (Object, Object, *const Arg) -> i64, sbn, post, core::ptr::null());
    let time = if env.test(package, |name| {
        matches!(name, b"android" | b"com.android.systemui")
    }) {
        env.string(c"system")
    } else {
        long_string(env, posted)
    };
    let identity = env.create(c"java/lang/StringBuilder", c"()V", &[]);
    let newline = env.string(c"\n");
    for part in [key, newline, time, newline, title, newline, body] {
        append(env, identity, part);
    }
    let id = hash(env, property(env, identity, c"toString"));
    let seen = env.get(owner, c"seen", SEEN);
    if env.call_bool(seen, c"contains", c"(Ljava/lang/Object;)Z", &[object(id)]) {
        return;
    }
    let event = env.create(c"org/json/JSONObject", c"()V", &[]);
    for (name, item) in [
        (c"event_id", id),
        (c"notification_key", bounded(env, key, 1024)),
        (c"package_name", package),
        (c"app_name", app_label(env, owner, package)),
        (c"title", title),
        (c"text", body),
    ] {
        json_put(env, event, name, OBJECT_VALUE, object(item));
    }
    json_put(
        env,
        event,
        c"posted_at_ms",
        LONG_VALUE,
        Arg { long: posted },
    );
    if !env.discard_exception() {
        enqueue(env, owner, event);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_onCreate(raw: RawEnv, owner: Object) {
    let env = Env(raw);
    superclass(env, owner, c"onCreate");
    let file = file(env, owner, c"notifications.json");
    let handler = handler(env);
    env.set(owner, c"file", FILE, file);
    env.set(owner, c"handler", HANDLER, handler);
    env.set(
        owner,
        c"events",
        QUEUE,
        env.create(c"java/util/ArrayDeque", c"()V", &[]),
    );
    env.set(
        owner,
        c"seen",
        SEEN,
        env.create(c"java/util/HashSet", c"()V", &[]),
    );
    if let Some(record) = saved(env, file) {
        let events = env.object(
            record,
            c"optJSONArray",
            c"(Ljava/lang/String;)Lorg/json/JSONArray;",
            &[object(env.string(c"notifications"))],
        );
        if !events.is_null() {
            let total = env.integer(events, c"length");
            for index in (total - 160).max(0)..total {
                if jni!(env, 19, (i32) -> i32, 32) != 0 {
                    break;
                }
                let event = env.object(
                    events,
                    c"getJSONObject",
                    c"(I)Lorg/json/JSONObject;",
                    &[int(index)],
                );
                let valid = !env.discard_exception();
                if valid {
                    enqueue(env, owner, event);
                }
                jni!(env, 20, (Object) -> Object, null_mut());
                if !valid {
                    break;
                }
            }
        }
    }
    env.call_bool(
        handler,
        c"post",
        c"(Ljava/lang/Runnable;)Z",
        &[object(owner)],
    );
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_onListenerConnected(
    raw: RawEnv,
    owner: Object,
) {
    let env = Env(raw);
    env.flag_set(owner, c"connected", true);
    let active = env.object(
        owner,
        c"getActiveNotifications",
        c"()[Landroid/service/notification/StatusBarNotification;",
        &[],
    );
    if !env.discard_exception() && !active.is_null() {
        for index in 0..jni!(env, 171, (Object) -> i32, active) {
            if jni!(env, 19, (i32) -> i32, 48) != 0 {
                break;
            }
            let sbn = jni!(env, 173, (Object, i32) -> Object, active, index);
            remember(env, owner, sbn);
            env.discard_exception();
            jni!(env, 20, (Object) -> Object, null_mut());
        }
    }
    persist(env, owner);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_onListenerDisconnected(
    raw: RawEnv,
    owner: Object,
) {
    let env = Env(raw);
    env.flag_set(owner, c"connected", false);
    persist(env, owner);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_onNotificationPosted(
    raw: RawEnv,
    owner: Object,
    sbn: Object,
) {
    let env = Env(raw);
    remember(env, owner, sbn);
    env.discard_exception();
    persist(env, owner);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_onNotificationRemoved(
    raw: RawEnv,
    owner: Object,
    _: Object,
) {
    persist(Env(raw), owner);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_onDestroy(raw: RawEnv, owner: Object) {
    let env = Env(raw);
    env.flag_set(owner, c"connected", false);
    env.void(
        env.get(owner, c"handler", HANDLER),
        c"removeCallbacksAndMessages",
        c"(Ljava/lang/Object;)V",
        &[object(null_mut())],
    );
    persist(env, owner);
    superclass(env, owner, c"onDestroy");
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_PhoneNotifications_run(raw: RawEnv, owner: Object) {
    let env = Env(raw);
    persist(env, owner);
    env.call_bool(
        env.get(owner, c"handler", HANDLER),
        c"postDelayed",
        c"(Ljava/lang/Runnable;J)Z",
        &[object(owner), Arg { long: 30_000 }],
    );
}
