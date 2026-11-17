#![no_std]

use core::ffi::{CStr, c_char, c_void};

type Object = *mut c_void;
type RawEnv = *const *const *const c_void;

#[repr(C)]
union Arg {
    object: Object,
    integer: i32,
    long: i64,
}

#[derive(Clone, Copy)]
struct Env(RawEnv);

// Indices are the stable JNI function table entries.
macro_rules! jni {
    ($env:expr, $slot:literal, ($($arg:ty),*) -> $ret:ty, $($value:expr),*) => {{
        let function: unsafe extern "system" fn(RawEnv, $($arg),*) -> $ret =
            unsafe { core::mem::transmute(*(*$env.0).add($slot)) };
        unsafe { function($env.0, $($value),*) }
    }};
}

mod attention;
mod feed;
mod hierarchy;
mod ime;
mod notifications;

impl Env {
    fn class(self, name: &CStr) -> Object {
        jni!(self, 6, (*const c_char) -> Object, name.as_ptr())
    }

    fn method(self, class: Object, name: &CStr, signature: &CStr) -> Object {
        jni!(self, 33, (Object, *const c_char, *const c_char) -> Object,
            class, name.as_ptr(), signature.as_ptr())
    }

    fn field(self, target: Object, name: &CStr, signature: &CStr) -> Object {
        let class = jni!(self, 31, (Object) -> Object, target);
        let field = jni!(self, 94, (Object, *const c_char, *const c_char) -> Object,
            class, name.as_ptr(), signature.as_ptr());
        self.release(class);
        field
    }

    fn instance_method(self, target: Object, name: &CStr, signature: &CStr) -> Object {
        let class = jni!(self, 31, (Object) -> Object, target);
        let method = self.method(class, name, signature);
        self.release(class);
        method
    }

    fn object(self, target: Object, name: &CStr, signature: &CStr, args: &[Arg]) -> Object {
        let method = self.instance_method(target, name, signature);
        self.call_object(target, method, args)
    }

    fn call_object(self, target: Object, method: Object, args: &[Arg]) -> Object {
        jni!(self, 36, (Object, Object, *const Arg) -> Object, target, method, args.as_ptr())
    }

    fn create(self, name: &CStr, signature: &CStr, args: &[Arg]) -> Object {
        let class = self.class(name);
        let method = self.method(class, c"<init>", signature);
        let value = jni!(self, 30, (Object, Object, *const Arg) -> Object,
            class, method, args.as_ptr());
        self.release(class);
        value
    }

    fn void(self, target: Object, name: &CStr, signature: &CStr, args: &[Arg]) {
        let method = self.instance_method(target, name, signature);
        jni!(self, 63, (Object, Object, *const Arg) -> (), target, method, args.as_ptr());
    }

    fn static_object(self, class: &CStr, name: &CStr, signature: &CStr, args: &[Arg]) -> Object {
        let class = self.class(class);
        let method = jni!(self, 113, (Object, *const c_char, *const c_char) -> Object,
            class, name.as_ptr(), signature.as_ptr());
        let result = jni!(self, 116, (Object, Object, *const Arg) -> Object,
            class, method, args.as_ptr());
        self.release(class);
        result
    }

    fn release(self, local: Object) {
        jni!(self, 23, (Object) -> (), local);
    }

    fn string(self, value: &CStr) -> Object {
        jni!(self, 167, (*const c_char) -> Object, value.as_ptr())
    }

    fn test(self, value: Object, predicate: impl FnOnce(&[u8]) -> bool) -> bool {
        if value.is_null() {
            return false;
        }
        let length = jni!(self, 168, (Object) -> i32, value);
        let bytes = jni!(self, 169, (Object, *mut u8) -> *const c_char,
            value, core::ptr::null_mut());
        if bytes.is_null() {
            return false;
        }
        let matches =
            predicate(unsafe { core::slice::from_raw_parts(bytes.cast(), length as usize) });
        jni!(self, 170, (Object, *const c_char) -> (), value, bytes);
        matches
    }

    fn discard_exception(self) -> bool {
        let exception = jni!(self, 15, () -> Object, );
        if exception.is_null() {
            return false;
        }
        jni!(self, 17, () -> (), );
        self.release(exception);
        true
    }

    fn extra(self, intent: Object, key: &CStr) -> Object {
        self.object(
            intent,
            c"getStringExtra",
            c"(Ljava/lang/String;)Ljava/lang/String;",
            &[Arg {
                object: self.string(key),
            }],
        )
    }

    fn service(self, context: Object, key: &CStr) -> Object {
        self.object(
            context,
            c"getSystemService",
            c"(Ljava/lang/String;)Ljava/lang/Object;",
            &[Arg {
                object: self.string(key),
            }],
        )
    }
}

fn access(env: Env, context: Object, intent: Object) -> Option<Object> {
    let service = env.extra(intent, c"service");
    if env.test(service, |name| name == b"notification") {
        let manager = env.service(context, c"notification");
        let class = env.class(c"android/content/ComponentName");
        let constructor = env.method(
            class,
            c"<init>",
            c"(Landroid/content/Context;Ljava/lang/String;)V",
        );
        let args = [
            Arg { object: context },
            Arg {
                object: env.string(c"dev.aiphone.input.PhoneNotifications"),
            },
        ];
        let component = jni!(env, 30, (Object, Object, *const Arg) -> Object,
            class, constructor, args.as_ptr());
        let method = env.instance_method(
            manager,
            c"isNotificationListenerAccessGranted",
            c"(Landroid/content/ComponentName;)Z",
        );
        let args = [Arg { object: component }];
        let ready =
            jni!(env, 39, (Object, Object, *const Arg) -> u8, manager, method, args.as_ptr()) != 0;
        return Some(env.string(if ready {
            c"notification:true"
        } else {
            c"notification:false"
        }));
    }
    if !env.test(service, |name| name == b"attention") {
        return None;
    }
    let manager = env.service(context, c"accessibility");
    let services = env.object(
        manager,
        c"getInstalledAccessibilityServiceList",
        c"()Ljava/util/List;",
        &[],
    );
    let size = env.instance_method(services, c"size", c"()I");
    let size =
        jni!(env, 51, (Object, Object, *const Arg) -> i32, services, size, core::ptr::null());
    for integer in 0..size {
        let service = env.object(
            services,
            c"get",
            c"(I)Ljava/lang/Object;",
            &[Arg { integer }],
        );
        let id = env.object(service, c"getId", c"()Ljava/lang/String;", &[]);
        let found = env.test(id, |id| {
            matches!(
                id,
                b"dev.aiphone.input/.PhoneAttention"
                    | b"dev.aiphone.input/dev.aiphone.input.PhoneAttention"
            )
        });
        env.release(id);
        env.release(service);
        if found {
            return Some(env.string(c"attention:true"));
        }
    }
    Some(env.string(c"attention:false"))
}

fn hierarchy(env: Env, intent: Object) -> Option<Object> {
    let request = env.extra(intent, c"request_id");
    if !env.test(request, |id| {
        id.len() == 32 && id.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }) {
        return None;
    }
    let root = env.static_object(
        c"dev/aiphone/input/PhoneAttention",
        c"activeRoot",
        c"()Landroid/view/accessibility/AccessibilityNodeInfo;",
        &[],
    );
    if root.is_null() {
        return None;
    }
    let xml = hierarchy::capture(env, root);
    env.discard_exception();
    let recycle = env.instance_method(root, c"recycle", c"()V");
    jni!(env, 63, (Object, Object, *const Arg) -> (), root, recycle, core::ptr::null());
    let xml = xml?;
    let bytes = env.object(
        xml,
        c"getBytes",
        c"(Ljava/lang/String;)[B",
        &[Arg {
            object: env.string(c"UTF-8"),
        }],
    );
    if jni!(env, 171, (Object) -> i32, bytes) > 262_144 {
        return None;
    }
    let encoded = env.static_object(
        c"android/util/Base64",
        c"encodeToString",
        c"([BI)Ljava/lang/String;",
        &[Arg { object: bytes }, Arg { integer: 2 }],
    );
    let concat = c"(Ljava/lang/String;)Ljava/lang/String;";
    let prefix = env.object(
        request,
        c"concat",
        concat,
        &[Arg {
            object: env.string(c":"),
        }],
    );
    Some(env.object(prefix, c"concat", concat, &[Arg { object: encoded }]))
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_UiTreeReceiver_onReceive(
    raw: RawEnv,
    receiver: Object,
    context: Object,
    intent: Object,
) {
    let env = Env(raw);
    let result_code = env.instance_method(receiver, c"setResultCode", c"(I)V");
    let cancelled = [Arg { integer: 0 }];
    jni!(env, 63, (Object, Object, *const Arg) -> (), receiver, result_code, cancelled.as_ptr());
    let action = env.object(intent, c"getAction", c"()Ljava/lang/String;", &[]);
    let reply = if env.test(action, |name| name == b"dev.aiphone.input.DUMP_UI") {
        hierarchy(env, intent).map(|data| (-1, data))
    } else if env.test(action, |name| name == b"dev.aiphone.input.READ_ACCESS") {
        access(env, context, intent).map(|data| (-1, data))
    } else if env.test(action, |name| name == b"dev.aiphone.input.INPUT") {
        ime::commit(env, receiver, intent)
    } else {
        None
    };
    if let Some((integer, object)) = reply {
        let result_data = env.instance_method(receiver, c"setResultData", c"(Ljava/lang/String;)V");
        let data = [Arg { object }];
        let ok = [Arg { integer }];
        jni!(env, 63, (Object, Object, *const Arg) -> (), receiver, result_data, data.as_ptr());
        jni!(env, 63, (Object, Object, *const Arg) -> (), receiver, result_code, ok.as_ptr());
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
