use super::{Arg, Env, Object, RawEnv};
use core::{ffi::CStr, ptr::null_mut};

const OWNER: &CStr = c"Ldev/aiphone/input/UnicodeIme;";
const RECEIVER: &CStr = c"Landroid/content/BroadcastReceiver;";

impl Env {
    fn ime_super(self, owner: Object, name: &CStr) {
        let class = self.class(c"android/inputmethodservice/InputMethodService");
        let method = self.method(class, name, c"()V");
        jni!(self, 93, (Object, Object, Object, *const Arg) -> (),
            owner, class, method, core::ptr::null());
        self.release(class);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_UnicodeIme_onCreate(raw: RawEnv, owner: Object) {
    let env = Env(raw);
    env.ime_super(owner, c"onCreate");
    let receiver = env.create(c"dev/aiphone/input/UiTreeReceiver", c"()V", &[]);
    let ime = env.field(receiver, c"imeOwner", OWNER);
    jni!(env, 104, (Object, Object, Object) -> (), receiver, ime, owner);
    let saved = env.field(owner, c"inputReceiver", RECEIVER);
    jni!(env, 104, (Object, Object, Object) -> (), owner, saved, receiver);
    let filter = env.create(
        c"android/content/IntentFilter",
        c"(Ljava/lang/String;)V",
        &[Arg {
            object: env.string(c"dev.aiphone.input.INPUT"),
        }],
    );
    env.object(owner, c"registerReceiver", c"(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;Ljava/lang/String;Landroid/os/Handler;I)Landroid/content/Intent;", &[
        Arg { object: receiver },
        Arg { object: filter },
        Arg { object: env.string(c"android.permission.DUMP") },
        Arg { object: null_mut() },
        Arg { integer: 2 },
    ]);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_UnicodeIme_onDestroy(raw: RawEnv, owner: Object) {
    let env = Env(raw);
    let saved = env.field(owner, c"inputReceiver", RECEIVER);
    let receiver = jni!(env, 95, (Object, Object) -> Object, owner, saved);
    env.void(
        owner,
        c"unregisterReceiver",
        c"(Landroid/content/BroadcastReceiver;)V",
        &[Arg { object: receiver }],
    );
    env.ime_super(owner, c"onDestroy");
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_UnicodeIme_onCreateInputView(
    raw: RawEnv,
    owner: Object,
) -> Object {
    let env = Env(raw);
    let label = env.create(
        c"android/widget/TextView",
        c"(Landroid/content/Context;)V",
        &[Arg { object: owner }],
    );
    env.void(
        label,
        c"setText",
        c"(Ljava/lang/CharSequence;)V",
        &[Arg {
            object: env.string(c""),
        }],
    );
    env.void(label, c"setHeight", c"(I)V", &[Arg { integer: 1 }]);
    label
}

pub(super) fn commit(env: Env, receiver: Object, intent: Object) -> Option<(i32, Object)> {
    let ime = env.field(receiver, c"imeOwner", OWNER);
    let owner = jni!(env, 95, (Object, Object) -> Object, receiver, ime);
    if owner.is_null() {
        return None;
    }
    let connection = env.object(
        owner,
        c"getCurrentInputConnection",
        c"()Landroid/view/inputmethod/InputConnection;",
        &[],
    );
    if connection.is_null() {
        return Some((2, env.string(c"no_active_editor")));
    }
    let payload = env.extra(intent, c"text_b64");
    if payload.is_null() || jni!(env, 164, (Object) -> i32, payload) > 24_000 {
        return Some((2, env.string(c"invalid_input")));
    }
    let bytes = env.static_object(
        c"android/util/Base64",
        c"decode",
        c"(Ljava/lang/String;I)[B",
        &[Arg { object: payload }, Arg { integer: 0 }],
    );
    if env.discard_exception() {
        return Some((2, env.string(c"invalid_input")));
    }
    let text = env.create(
        c"java/lang/String",
        c"([BLjava/lang/String;)V",
        &[
            Arg { object: bytes },
            Arg {
                object: env.string(c"UTF-8"),
            },
        ],
    );
    let method = env.instance_method(connection, c"commitText", c"(Ljava/lang/CharSequence;I)Z");
    let args = [Arg { object: text }, Arg { integer: 1 }];
    let accepted =
        jni!(env, 39, (Object, Object, *const Arg) -> u8, connection, method, args.as_ptr());
    if env.discard_exception() {
        return Some((2, env.string(c"invalid_input")));
    }
    if accepted == 0 {
        return Some((2, env.string(c"editor_rejected")));
    }
    let action = env.extra(intent, c"action_id");
    let action = if action.is_null() {
        env.string(c"null")
    } else {
        action
    };
    let receipt = env.object(
        env.string(c"committed:"),
        c"concat",
        c"(Ljava/lang/String;)Ljava/lang/String;",
        &[Arg { object: action }],
    );
    Some((1, receipt))
}
