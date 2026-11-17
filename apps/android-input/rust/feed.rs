use super::{Arg, Env, Object, RawEnv};
use core::ffi::{CStr, c_char};

pub(super) const HANDLER: &CStr = c"Landroid/os/Handler;";
pub(super) const FILE: &CStr = c"Landroid/util/AtomicFile;";
pub(super) const STRING: &CStr = c"Ljava/lang/String;";
pub(super) const ARRAY: &CStr = c"Lorg/json/JSONArray;";
pub(super) const SEQUENCE: &CStr = c"()Ljava/lang/CharSequence;";
pub(super) const STRING_GET: &CStr = c"()Ljava/lang/String;";

pub(super) fn object(value: Object) -> Arg {
    Arg { object: value }
}
pub(super) fn int(value: i32) -> Arg {
    Arg { integer: value }
}

impl Env {
    pub(super) fn get(self, target: Object, name: &CStr, kind: &CStr) -> Object {
        let field = self.field(target, name, kind);
        jni!(self, 95, (Object, Object) -> Object, target, field)
    }

    pub(super) fn set(self, target: Object, name: &CStr, kind: &CStr, value: Object) {
        let field = self.field(target, name, kind);
        jni!(self, 104, (Object, Object, Object) -> (), target, field, value);
    }

    pub(super) fn flag(self, target: Object, name: &CStr) -> bool {
        let field = self.field(target, name, c"Z");
        jni!(self, 96, (Object, Object) -> u8, target, field) != 0
    }

    pub(super) fn flag_set(self, target: Object, name: &CStr, value: bool) {
        let field = self.field(target, name, c"Z");
        jni!(self, 105, (Object, Object, u8) -> (), target, field, u8::from(value));
    }

    pub(super) fn call_bool(
        self,
        target: Object,
        name: &CStr,
        signature: &CStr,
        args: &[Arg],
    ) -> bool {
        let method = self.instance_method(target, name, signature);
        jni!(self, 39, (Object, Object, *const Arg) -> u8, target, method, args.as_ptr()) != 0
    }

    pub(super) fn integer(self, target: Object, name: &CStr) -> i32 {
        let method = self.instance_method(target, name, c"()I");
        jni!(self, 51, (Object, Object, *const Arg) -> i32, target, method, core::ptr::null())
    }

    pub(super) fn int_field(self, target: Object, name: &CStr) -> i32 {
        let field = self.field(target, name, c"I");
        jni!(self, 100, (Object, Object) -> i32, target, field)
    }

    pub(super) fn text(self, target: Object, name: &CStr) -> Object {
        let value = self.object(target, name, SEQUENCE, &[]);
        if value.is_null() {
            return self.string(c"");
        }
        let result = self.object(value, c"toString", STRING_GET, &[]);
        self.release(value);
        result
    }

    pub(super) fn bytes(self, text: Object) -> Object {
        self.object(
            text,
            c"getBytes",
            c"(Ljava/lang/String;)[B",
            &[object(self.string(c"UTF-8"))],
        )
    }

    pub(super) fn utf8(self, bytes: Object) -> Object {
        self.create(
            c"java/lang/String",
            c"([BLjava/lang/String;)V",
            &[object(bytes), object(self.string(c"UTF-8"))],
        )
    }
}

pub(super) fn now(env: Env) -> i64 {
    let class = env.class(c"java/lang/System");
    let method = jni!(env, 113, (Object, *const c_char, *const c_char) -> Object,
        class, c"currentTimeMillis".as_ptr(), c"()J".as_ptr());
    jni!(env, 134, (Object, Object, *const Arg) -> i64, class, method, core::ptr::null())
}

pub(super) fn json_put(env: Env, target: Object, name: &CStr, signature: &CStr, value: Arg) {
    env.release(env.object(
        target,
        c"put",
        signature,
        &[object(env.string(name)), value],
    ));
}

pub(super) fn append(env: Env, builder: Object, value: Object) {
    env.release(env.object(
        builder,
        c"append",
        c"(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        &[object(value)],
    ));
}

pub(super) fn hash(env: Env, value: Object) -> Object {
    let digest = env.static_object(
        c"java/security/MessageDigest",
        c"getInstance",
        c"(Ljava/lang/String;)Ljava/security/MessageDigest;",
        &[object(env.string(c"SHA-256"))],
    );
    let bytes = env.object(digest, c"digest", c"([B)[B", &[object(env.bytes(value))]);
    let mut content = [0u8; 32];
    jni!(env, 200, (Object, i32, i32, *mut u8) -> (), bytes, 0, 32, content.as_mut_ptr());
    let mut out = [0u8; 65];
    for (pair, value) in out.chunks_exact_mut(2).zip(content) {
        let [high, low] = pair else {
            continue;
        };
        for (digit, output) in [(value >> 4, high), (value & 15, low)] {
            *output = if digit < 10 {
                b'0' + digit
            } else {
                b'a' + digit - 10
            };
        }
    }
    jni!(env, 167, (*const c_char) -> Object, out.as_ptr().cast())
}

pub(super) fn long_string(env: Env, value: i64) -> Object {
    env.static_object(
        c"java/lang/Long",
        c"toString",
        c"(J)Ljava/lang/String;",
        &[Arg { long: value }],
    )
}

pub(super) fn file(env: Env, owner: Object, name: &CStr) -> Object {
    let directory = env.object(owner, c"getFilesDir", c"()Ljava/io/File;", &[]);
    let path = env.create(
        c"java/io/File",
        c"(Ljava/io/File;Ljava/lang/String;)V",
        &[object(directory), object(env.string(name))],
    );
    env.create(
        c"android/util/AtomicFile",
        c"(Ljava/io/File;)V",
        &[object(path)],
    )
}

pub(super) fn handler(env: Env) -> Object {
    let looper = env.static_object(
        c"android/os/Looper",
        c"getMainLooper",
        c"()Landroid/os/Looper;",
        &[],
    );
    env.create(
        c"android/os/Handler",
        c"(Landroid/os/Looper;)V",
        &[object(looper)],
    )
}

pub(super) fn saved(env: Env, file: Object) -> Option<Object> {
    let bytes = env.object(file, c"readFully", c"()[B", &[]);
    if env.discard_exception() {
        return None;
    }
    let value = env.create(
        c"org/json/JSONObject",
        c"(Ljava/lang/String;)V",
        &[object(env.utf8(bytes))],
    );
    (!env.discard_exception()).then_some(value)
}

pub(super) fn write(env: Env, file: Object, record: Object) {
    let output = env.bytes(env.object(record, c"toString", STRING_GET, &[]));
    let stream = env.object(file, c"startWrite", c"()Ljava/io/FileOutputStream;", &[]);
    if env.discard_exception() {
        return;
    }
    env.void(stream, c"write", c"([B)V", &[object(output)]);
    let method = if env.discard_exception() {
        c"failWrite"
    } else {
        c"finishWrite"
    };
    env.void(
        file,
        method,
        c"(Ljava/io/FileOutputStream;)V",
        &[object(stream)],
    );
    env.discard_exception();
}
