#![no_std]

use core::{
    ffi::{CStr, c_char, c_void},
    ptr,
};

type Object = *mut c_void;
type Raw = *const *const *const c_void;

#[repr(C)]
union Arg {
    object: Object,
    integer: i32,
}

macro_rules! jni {
    ($env:expr, $slot:literal, ($($arg:ty),*) -> $ret:ty $(, $value:expr)*) => {{
        let function: unsafe extern "system" fn(Raw, $($arg),*) -> $ret =
            unsafe { core::mem::transmute(*(*$env).add($slot)) };
        unsafe { function($env, $($value),*) }
    }};
}

fn class(env: Raw, name: &CStr) -> Object {
    jni!(env, 6, (*const c_char) -> Object, name.as_ptr())
}

fn string(env: Raw, value: &CStr) -> Object {
    jni!(env, 167, (*const c_char) -> Object, value.as_ptr())
}

fn method(env: Raw, class: Object, name: &CStr, signature: &CStr) -> Object {
    jni!(env, 33, (Object, *const c_char, *const c_char) -> Object,
        class, name.as_ptr(), signature.as_ptr())
}

fn field(env: Raw, class: Object, name: &CStr, signature: &CStr) -> Object {
    jni!(env, 94, (Object, *const c_char, *const c_char) -> Object,
        class, name.as_ptr(), signature.as_ptr())
}

fn static_object(
    env: Raw,
    name: &CStr,
    function: &CStr,
    signature: &CStr,
    argument: Object,
) -> Object {
    let class = class(env, name);
    let method = jni!(env, 113, (Object, *const c_char, *const c_char) -> Object,
        class, function.as_ptr(), signature.as_ptr());
    jni!(env, 116, (Object, Object, *const Arg) -> Object, class, method, &[Arg{object:argument}] as *const Arg)
}

fn array_length(env: Raw, array: Object) -> i32 {
    jni!(env, 171, (Object) -> i32, array)
}

fn report(env: Raw, count: i32, changed: i32) {
    let builder = class(env, c"java/lang/StringBuilder");
    let init = method(env, builder, c"<init>", c"()V");
    let line = jni!(env, 30, (Object, Object, *const Arg) -> Object, builder, init, ptr::null());
    let append = method(
        env,
        builder,
        c"append",
        c"(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
    let number = method(env, builder, c"append", c"(I)Ljava/lang/StringBuilder;");
    for (label, integer) in [
        (c"DATA_USAGE_WARNINGS policies=", count),
        (c" enabled=0 inferred=0 changed=", changed),
    ] {
        for (append, arg) in [
            (
                append,
                Arg {
                    object: string(env, label),
                },
            ),
            (number, Arg { integer }),
        ] {
            let own = jni!(env, 36, (Object, Object, *const Arg) -> Object, line, append, &arg);
            jni!(env, 23, (Object) -> (), own);
        }
    }
    let to_string = method(env, builder, c"toString", c"()Ljava/lang/String;");
    let text = jni!(env, 36, (Object, Object, *const Arg) -> Object, line, to_string, ptr::null());
    let system = class(env, c"java/lang/System");
    let out = jni!(env, 144, (Object, *const c_char, *const c_char) -> Object,
        system, c"out".as_ptr(), c"Ljava/io/PrintStream;".as_ptr());
    let output = jni!(env, 145, (Object, Object) -> Object, system, out);
    let println = method(
        env,
        class(env, c"java/io/PrintStream"),
        c"println",
        c"(Ljava/lang/String;)V",
    );
    jni!(env, 63, (Object, Object, *const Arg) -> (), output, println, &Arg { object: text });
}

fn suppress(env: Raw) -> Option<(i32, i32)> {
    let binder = static_object(
        env,
        c"android/os/ServiceManager",
        c"getService",
        c"(Ljava/lang/String;)Landroid/os/IBinder;",
        string(env, c"netpolicy"),
    );
    if binder.is_null() {
        return None;
    }
    let service = static_object(
        env,
        c"android/net/INetworkPolicyManager$Stub",
        c"asInterface",
        c"(Landroid/os/IBinder;)Landroid/net/INetworkPolicyManager;",
        binder,
    );
    if service.is_null() {
        return None;
    }
    let interface = class(env, c"android/net/INetworkPolicyManager");
    let read = method(
        env,
        interface,
        c"getNetworkPolicies",
        c"(Ljava/lang/String;)[Landroid/net/NetworkPolicy;",
    );
    let package = Arg {
        object: string(env, c"android"),
    };
    let policies = jni!(env, 36, (Object, Object, *const Arg) -> Object, service, read, &package);
    if policies.is_null() {
        return None;
    }
    let policy_type = class(env, c"android/net/NetworkPolicy");
    let warning = field(env, policy_type, c"warningBytes", c"J");
    let inferred = field(env, policy_type, c"inferred", c"Z");
    let count = array_length(env, policies);
    let differs = |policy| {
        let bytes = jni!(env, 101, (Object, Object) -> i64, policy, warning);
        let automatic = jni!(env, 96, (Object, Object) -> u8, policy, inferred);
        bytes != -1 || automatic != 0
    };
    let mut changed = 0;
    for index in 0..count {
        let policy = jni!(env, 173, (Object, i32) -> Object, policies, index);
        if differs(policy) {
            jni!(env, 110, (Object, Object, i64) -> (), policy, warning, -1);
            jni!(env, 105, (Object, Object, u8) -> (), policy, inferred, 0);
            changed += 1;
        }
        jni!(env, 23, (Object) -> (), policy);
    }
    if changed > 0 {
        let write = method(
            env,
            interface,
            c"setNetworkPolicies",
            c"([Landroid/net/NetworkPolicy;)V",
        );
        jni!(env, 63, (Object, Object, *const Arg) -> (), service, write, &Arg { object: policies });
        if jni!(env, 228, () -> u8) != 0 {
            return None;
        }
        let current =
            jni!(env, 36, (Object, Object, *const Arg) -> Object, service, read, &package);
        if current.is_null() || array_length(env, current) != count {
            return None;
        }
        for index in 0..count {
            let policy = jni!(env, 173, (Object, i32) -> Object, current, index);
            let differs = differs(policy);
            jni!(env, 23, (Object) -> (), policy);
            if differs {
                return None;
            }
        }
    }
    Some((count, changed))
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_network_DataUsageWarnings_main(
    env: Raw,
    _: Object,
    args: Object,
) {
    let expected = string(env, c"--disable-warnings");
    let equals = method(
        env,
        class(env, c"java/lang/String"),
        c"equals",
        c"(Ljava/lang/Object;)Z",
    );
    let valid = array_length(env, args) == 1 && {
        let object = jni!(env, 173, (Object, i32) -> Object, args, 0);
        jni!(env, 39, (Object, Object, *const Arg) -> u8, expected, equals, &Arg { object }) != 0
    };
    if !valid {
        jni!(env, 14, (Object, *const c_char) -> i32,
            class(env, c"java/lang/IllegalArgumentException"), c"Expected --disable-warnings".as_ptr());
        return;
    }
    if let Some((count, changed)) = suppress(env) {
        report(env, count, changed);
    } else if jni!(env, 228, () -> u8) == 0 {
        jni!(env, 14, (Object, *const c_char) -> i32,
            class(env, c"java/lang/IllegalStateException"), c"Usage warning readback mismatch".as_ptr());
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
