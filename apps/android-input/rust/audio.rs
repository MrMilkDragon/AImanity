use super::{
    Arg, Env, Object, RawEnv,
    feed::{int, object},
};
use core::ffi::{CStr, c_char};

const READY: i32 = 0x41554d31;
const DONE: i32 = 0x444f4e45;
const BLOCK: i32 = 8192;

fn number(env: Env, receiver: Object, name: &CStr, signature: &CStr, args: &[Arg]) -> i32 {
    let method = env.instance_method(receiver, name, signature);
    jni!(env, 51, (Object, Object, *const Arg) -> i32, receiver, method, args.as_ptr())
}

fn descriptor(env: Env, name: &CStr) -> Object {
    let class = env.class(c"java/io/FileDescriptor");
    let field = jni!(env, 144, (Object, *const c_char, *const c_char) -> Object, class, name.as_ptr(), c"Ljava/io/FileDescriptor;".as_ptr());
    jni!(env, 145, (Object, Object) -> Object, class, field)
}

fn policy(env: Env, context: Object, input: bool) -> Option<(Object, Object, Object)> {
    let rule = env.create(
        c"android/media/audiopolicy/AudioMixingRule$Builder",
        c"()V",
        &[],
    );
    let presets: &[i32] = if input {
        &[0, 1, 5, 6, 7, 9, 10]
    } else {
        &[15]
    };
    for &preset in presets {
        let builder = env.create(c"android/media/AudioAttributes$Builder", c"()V", &[]);
        let setter = if input {
            c"setCapturePreset"
        } else {
            c"setUsage"
        };
        env.object(
            builder,
            setter,
            c"(I)Landroid/media/AudioAttributes$Builder;",
            &[int(preset)],
        );
        let attr = env.object(builder, c"build", c"()Landroid/media/AudioAttributes;", &[]);
        let operation = if input { c"addRule" } else { c"excludeRule" };
        // USAGE_VIRTUAL_SOURCE belongs to the microphone injector. Never capture it as speaker output.
        env.object(rule, operation, c"(Landroid/media/AudioAttributes;I)Landroid/media/audiopolicy/AudioMixingRule$Builder;",
            &[object(attr), int(if input { 2 } else { 1 })]);
    }
    let rule = env.object(
        rule,
        c"build",
        c"()Landroid/media/audiopolicy/AudioMixingRule;",
        &[],
    );
    if rule.is_null() {
        return None;
    }
    let format = env.create(c"android/media/AudioFormat$Builder", c"()V", &[]);
    for (property, value) in [
        (c"setEncoding", 2),
        (c"setSampleRate", 48000),
        (c"setChannelMask", if input { 4 } else { 12 }),
    ] {
        env.object(
            format,
            property,
            c"(I)Landroid/media/AudioFormat$Builder;",
            &[int(value)],
        );
    }
    let format = env.object(format, c"build", c"()Landroid/media/AudioFormat;", &[]);
    let mix = env.create(
        c"android/media/audiopolicy/AudioMix$Builder",
        c"(Landroid/media/audiopolicy/AudioMixingRule;)V",
        &[object(rule)],
    );
    env.object(
        mix,
        c"setFormat",
        c"(Landroid/media/AudioFormat;)Landroid/media/audiopolicy/AudioMix$Builder;",
        &[object(format)],
    );
    env.object(
        mix,
        c"setRouteFlags",
        c"(I)Landroid/media/audiopolicy/AudioMix$Builder;",
        &[int(2)],
    );
    let mix = env.object(
        mix,
        c"build",
        c"()Landroid/media/audiopolicy/AudioMix;",
        &[],
    );
    if mix.is_null() {
        return None;
    }
    let builder = env.create(
        c"android/media/audiopolicy/AudioPolicy$Builder",
        c"(Landroid/content/Context;)V",
        &[object(context)],
    );
    env.object(
        builder,
        c"addMix",
        c"(Landroid/media/audiopolicy/AudioMix;)Landroid/media/audiopolicy/AudioPolicy$Builder;",
        &[object(mix)],
    );
    let policy = env.object(
        builder,
        c"build",
        c"()Landroid/media/audiopolicy/AudioPolicy;",
        &[],
    );
    let audio = env.service(context, c"audio");
    if policy.is_null()
        || audio.is_null()
        || number(
            env,
            audio,
            c"registerAudioPolicy",
            c"(Landroid/media/audiopolicy/AudioPolicy;)I",
            &[object(policy)],
        ) != 0
        || env.discard_exception()
    {
        return None;
    }
    Some((audio, policy, mix))
}

fn transfer(env: Env, endpoint: Object, input: bool) -> bool {
    let out = env.create(
        c"java/io/FileOutputStream",
        c"(Ljava/io/FileDescriptor;)V",
        &[object(descriptor(env, c"out"))],
    );
    let out = env.create(
        c"java/io/DataOutputStream",
        c"(Ljava/io/OutputStream;)V",
        &[object(out)],
    );
    let data = jni!(env, 176, (i32) -> Object, BLOCK);
    env.void(
        endpoint,
        if input { c"play" } else { c"startRecording" },
        c"()V",
        &[],
    );
    if env.discard_exception() {
        return false;
    }
    env.void(out, c"writeInt", c"(I)V", &[int(READY)]);
    if input {
        let source = env.create(
            c"java/io/FileInputStream",
            c"(Ljava/io/FileDescriptor;)V",
            &[object(descriptor(env, c"in"))],
        );
        let source = env.create(
            c"java/io/DataInputStream",
            c"(Ljava/io/InputStream;)V",
            &[object(source)],
        );
        let mut frames = 0u32;
        loop {
            let size = env.integer(source, c"readInt");
            if env.discard_exception() || !(0..=BLOCK).contains(&size) || size % 2 != 0 {
                return false;
            }
            if size == 0 {
                break;
            }
            env.void(
                source,
                c"readFully",
                c"([BII)V",
                &[object(data), int(0), int(size)],
            );
            if env.discard_exception() {
                return false;
            }
            let mut offset = 0;
            while offset < size {
                let n = number(
                    env,
                    endpoint,
                    c"write",
                    c"([BII)I",
                    &[object(data), int(offset), int(size - offset)],
                );
                if n <= 0 || env.discard_exception() {
                    return false;
                }
                offset += n;
            }
            frames = frames.wrapping_add((size / 2) as u32);
        }
        let thread = env.class(c"java/lang/Thread");
        let sleep = jni!(env, 113, (Object,*const c_char,*const c_char)->Object, thread,c"sleep".as_ptr(),c"(J)V".as_ptr());
        for _ in 0..200 {
            if env.integer(endpoint, c"getPlaybackHeadPosition") as u32 == frames {
                env.void(out, c"writeInt", c"(I)V", &[int(DONE)]);
                return !env.discard_exception();
            }
            jni!(env, 143, (Object,Object,*const Arg)->(), thread,sleep,&Arg{long:10});
        }
    } else {
        loop {
            let count = number(
                env,
                endpoint,
                c"read",
                c"([BII)I",
                &[object(data), int(0), int(BLOCK)],
            );
            if count <= 0 || env.discard_exception() {
                break;
            }
            env.void(
                out,
                c"write",
                c"([BII)V",
                &[object(data), int(0), int(count)],
            );
            if env.discard_exception() {
                break;
            }
        }
    }
    false
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_aiphone_input_VirtualAudio_main(raw: RawEnv, _: Object, args: Object) {
    let env = Env(raw);
    if jni!(env, 171, (Object) -> i32, args) != 1 {
        return;
    }
    let mode = jni!(env, 173, (Object,i32) -> Object, args, 0);
    let input = env.test(mode, |value| value == b"mic");
    if !input && !env.test(mode, |value| value == b"speaker") {
        return;
    }
    let looper = env.class(c"android/os/Looper");
    if env
        .static_object(
            c"android/os/Looper",
            c"getMainLooper",
            c"()Landroid/os/Looper;",
            &[],
        )
        .is_null()
    {
        let init = jni!(env,113,(Object,*const c_char,*const c_char)->Object,looper,c"prepareMainLooper".as_ptr(),c"()V".as_ptr());
        jni!(env,143,(Object,Object,*const Arg)->(),looper,init,core::ptr::null());
    }
    let activity = env.static_object(
        c"android/app/ActivityThread",
        c"systemMain",
        c"()Landroid/app/ActivityThread;",
        &[],
    );
    if activity.is_null() {
        return;
    }
    let context = env.object(
        activity,
        c"getSystemContext",
        c"()Landroid/app/ContextImpl;",
        &[],
    );
    let Some((manager, policy, mix)) = policy(env, context, input) else {
        return;
    };
    let endpoint = if input {
        env.object(
            policy,
            c"createAudioTrackSource",
            c"(Landroid/media/audiopolicy/AudioMix;)Landroid/media/AudioTrack;",
            &[object(mix)],
        )
    } else {
        env.object(
            policy,
            c"createAudioRecordSink",
            c"(Landroid/media/audiopolicy/AudioMix;)Landroid/media/AudioRecord;",
            &[object(mix)],
        )
    };
    if !endpoint.is_null() && env.integer(endpoint, c"getState") == 1 {
        transfer(env, endpoint, input);
        env.void(endpoint, c"release", c"()V", &[]);
    }
    env.discard_exception();
    env.void(
        manager,
        c"unregisterAudioPolicy",
        c"(Landroid/media/audiopolicy/AudioPolicy;)V",
        &[object(policy)],
    );
}
