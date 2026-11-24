use ring::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use std::collections::{BTreeMap, BTreeSet};

type Name = &'static str;
type Types = &'static [Name];
type Proto = (Name, Types);
type Method = (Name, Name, Proto);

const VOID: Proto = ("V", &[]);
const STRING: Name = "Ljava/lang/String;";
const SYSTEM: Name = "Ljava/lang/System;";
const RECEIVER: Name = "Landroid/content/BroadcastReceiver;";
const IME: Name = "Ldev/aiphone/input/UnicodeIme;";
const ATTENTION: Name = "Ldev/aiphone/input/PhoneAttention;";
const TIMER: Name = "Ldev/aiphone/input/AttentionTimer;";
const SCREENSHOT: Name = "Ldev/aiphone/input/AttentionScreenshot;";
const RECT: Name = "Landroid/graphics/Rect;";
const SBN: Types = &["Landroid/service/notification/StatusBarNotification;"];

pub(super) struct Class {
    pub name: Name,
    pub parent: Name,
    pub interfaces: Types,
    pub constructor_args: Types,
    pub fields: &'static [(Name, Name, u32)],
    pub natives: &'static [(Name, Proto, u32)],
}

impl Class {
    fn constructor(&self) -> Proto {
        ("V", self.constructor_args)
    }
}

const AUDIO: Name = "Ldev/aiphone/input/VirtualAudio;";
const AUDIO_LIB: Name = "aimanity.audio.library";
const CLASSES: &[Class] = &[
    Class {
        name: AUDIO,
        parent: "Ljava/lang/Object;",
        constructor_args: &[],
        interfaces: &[],
        fields: &[],
        natives: &[("main", ("V", &["[Ljava/lang/String;"]), 9)],
    },
    Class {
        name: "Ldev/aiphone/input/PhoneNotifications;",
        parent: "Landroid/service/notification/NotificationListenerService;",
        constructor_args: &[],
        interfaces: &["Ljava/lang/Runnable;"],
        fields: &[
            ("file", "Landroid/util/AtomicFile;", 2),
            ("handler", "Landroid/os/Handler;", 2),
            ("events", "Ljava/util/ArrayDeque;", 2),
            ("seen", "Ljava/util/HashSet;", 2),
            ("connected", "Z", 2),
            ("bufferedBytes", "I", 2),
        ],
        natives: &[
            ("onCreate", VOID, 1),
            ("onDestroy", VOID, 1),
            ("run", VOID, 1),
            ("onListenerConnected", VOID, 1),
            ("onListenerDisconnected", VOID, 1),
            ("onNotificationPosted", ("V", SBN), 1),
            ("onNotificationRemoved", ("V", SBN), 1),
        ],
    },
    Class {
        name: "Ldev/aiphone/input/UiTreeReceiver;",
        parent: RECEIVER,
        constructor_args: &[],
        interfaces: &[],
        fields: &[("imeOwner", IME, 2)],
        natives: &[(
            "onReceive",
            (
                "V",
                &["Landroid/content/Context;", "Landroid/content/Intent;"],
            ),
            1,
        )],
    },
    Class {
        name: IME,
        parent: "Landroid/inputmethodservice/InputMethodService;",
        constructor_args: &[],
        interfaces: &[],
        fields: &[("inputReceiver", RECEIVER, 2)],
        natives: &[
            ("onCreate", VOID, 1),
            ("onCreateInputView", ("Landroid/view/View;", &[]), 1),
            ("onDestroy", VOID, 1),
        ],
    },
    Class {
        name: ATTENTION,
        parent: "Landroid/accessibilityservice/AccessibilityService;",
        constructor_args: &[],
        interfaces: &[],
        fields: &[
            ("active", ATTENTION, 10),
            ("handler", "Landroid/os/Handler;", 2),
            ("scan", TIMER, 2),
            ("heartbeat", TIMER, 2),
            ("file", "Landroid/util/AtomicFile;", 2),
            ("events", "Lorg/json/JSONArray;", 2),
            ("signature", STRING, 2),
            ("connected", "Z", 2),
            ("capturing", "Z", 2),
            ("chatBadge", "Z", 2),
            ("contactsBadge", "Z", 2),
        ],
        natives: &[
            (
                "activeRoot",
                ("Landroid/view/accessibility/AccessibilityNodeInfo;", &[]),
                8,
            ),
            ("onServiceConnected", VOID, 4),
            (
                "onAccessibilityEvent",
                ("V", &["Landroid/view/accessibility/AccessibilityEvent;"]),
                1,
            ),
            ("onInterrupt", VOID, 1),
            ("onDestroy", VOID, 1),
            ("onUnbind", ("Z", &["Landroid/content/Intent;"]), 1),
        ],
    },
    Class {
        name: TIMER,
        parent: "Ljava/lang/Object;",
        constructor_args: &[],
        interfaces: &["Ljava/lang/Runnable;"],
        fields: &[("owner", ATTENTION, 2), ("heartbeat", "Z", 2)],
        natives: &[("run", VOID, 1)],
    },
    Class {
        name: SCREENSHOT,
        parent: "Ljava/lang/Object;",
        constructor_args: &[],
        interfaces: &["Landroid/accessibilityservice/AccessibilityService$TakeScreenshotCallback;"],
        fields: &[
            ("owner", ATTENTION, 2),
            ("chat", RECT, 2),
            ("contacts", RECT, 2),
        ],
        natives: &[
            (
                "onSuccess",
                (
                    "V",
                    &["Landroid/accessibilityservice/AccessibilityService$ScreenshotResult;"],
                ),
                1,
            ),
            ("onFailure", ("V", &["I"]), 1),
        ],
    },
];

fn write(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn push(buf: &mut Vec<u8>, values: &[u32]) {
    buf.extend(values.iter().flat_map(|value| value.to_le_bytes()));
}

fn leb(buf: &mut Vec<u8>, mut value: u32) {
    while value > 0x7f {
        buf.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

fn code(dex: &mut Vec<u8>, incoming: u32, instructions: &[u16]) -> u32 {
    let start = dex.len() as u32;
    push(
        dex,
        &[
            incoming.max(1) | (incoming << 16),
            incoming.max(1),
            0,
            instructions.len() as u32,
        ],
    );
    dex.extend(instructions.iter().flat_map(|insn| insn.to_le_bytes()));
    start
}

pub(super) fn emit() -> Vec<u8> {
    emit_for(CLASSES, "aimanity_input")
}

pub(super) fn emit_for(schema: &[Class], library: Name) -> Vec<u8> {
    let loader = if library.starts_with('/') {
        "load"
    } else {
        "loadLibrary"
    };
    let load: Method = (SYSTEM, loader, ("V", &[STRING]));
    let mut classes: Vec<_> = schema.iter().collect();
    classes.sort_unstable_by_key(|class| class.name);
    let mut methods = BTreeSet::from([load]);
    let absolute: Method = (SYSTEM, "load", ("V", &[STRING]));
    let property: Method = (SYSTEM, "getProperty", (STRING, &[STRING]));
    let audio = classes.iter().any(|c| c.name == AUDIO);
    if audio {
        methods.extend([absolute, property]);
    }
    let mut fields = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut lists = BTreeSet::new();
    for class in &classes {
        types.extend(class.interfaces.iter().copied());
        lists.insert(class.interfaces);
        methods.extend([
            (class.parent, "<init>", class.constructor()),
            (class.name, "<init>", class.constructor()),
            (class.name, "<clinit>", VOID),
        ]);
        fields.extend(
            class
                .fields
                .iter()
                .map(|&(name, ty, _)| (class.name, name, ty)),
        );
        methods.extend(
            class
                .natives
                .iter()
                .map(|&(name, proto, _)| (class.name, name, proto)),
        );
    }
    let methods: Vec<_> = methods.into_iter().collect();
    let fields: Vec<_> = fields.into_iter().collect();
    let protos: BTreeSet<_> = methods.iter().map(|&(_, _, proto)| proto).collect();
    let protos: Vec<_> = protos.into_iter().collect();
    let shorties: Vec<String> = protos
        .iter()
        .map(|&(result, args)| {
            std::iter::once(result)
                .chain(args.iter().copied())
                .map(|ty| {
                    let base = ty.as_bytes()[0];
                    if base == b'[' { 'L' } else { char::from(base) }
                })
                .collect()
        })
        .collect();
    let mut strings = BTreeSet::from([library]);
    if audio {
        strings.insert(AUDIO_LIB);
    }
    for &(class, name, (result, params)) in &methods {
        types.extend([class, result]);
        types.extend(params.iter().copied());
        strings.insert(name);
        lists.insert(params);
    }
    for &(_, name, ty) in &fields {
        strings.insert(name);
        types.insert(ty);
    }
    strings.extend(types.iter().copied());
    strings.extend(shorties.iter().map(String::as_str));
    let types: Vec<_> = types.into_iter().collect();
    let strings: Vec<_> = strings.into_iter().collect();
    let string = |name| strings.binary_search(&name).unwrap() as u32;
    let ty = |name| types.binary_search(&name).unwrap() as u32;
    let method = |key| methods.binary_search(&key).unwrap() as u32;
    let field = |key| fields.binary_search(&key).unwrap() as u32;

    let mut dex = vec![0; 112];
    dex[..8].copy_from_slice(b"dex\n035\0");
    let mut sections = vec![(0x0000, 1, 0)];
    for (kind, count, width) in [
        (1, strings.len(), 4),
        (2, types.len(), 4),
        (3, protos.len(), 12),
        (4, fields.len(), 8),
        (5, methods.len(), 8),
        (6, classes.len(), 32),
    ] {
        let start = dex.len();
        sections.push((kind, count as u32, start as u32));
        dex.resize(start + count * width, 0);
        write(&mut dex, 56 + (kind - 1) as usize * 8, count as u32);
        write(
            &mut dex,
            60 + (kind - 1) as usize * 8,
            if count == 0 { 0 } else { start as u32 },
        );
    }
    let data = dex.len() as u32;
    lists.remove(&[][..]);
    sections.push((0x1001, lists.len() as u32, data));
    let mut offsets = BTreeMap::new();
    for names in lists {
        offsets.insert(names, dex.len() as u32);
        push(&mut dex, &[names.len() as u32]);
        dex.extend(
            names
                .iter()
                .flat_map(|name| (ty(name) as u16).to_le_bytes()),
        );
        dex.resize(dex.len().next_multiple_of(4), 0);
    }
    let type_list = |names: Types| if names.is_empty() { 0 } else { offsets[names] };
    let string_at = sections[1].2 as usize;
    sections.push((0x2002, strings.len() as u32, dex.len() as u32));
    for (index, value) in strings.iter().enumerate() {
        let start = dex.len() as u32;
        write(&mut dex, string_at + index * 4, start);
        leb(&mut dex, value.len() as u32);
        dex.extend_from_slice(value.as_bytes());
        dex.push(0);
    }
    for (index, name) in types.iter().enumerate() {
        write(&mut dex, sections[2].2 as usize + index * 4, string(name));
    }
    for (index, (&(result, params), shorty)) in protos.iter().zip(&shorties).enumerate() {
        let at = sections[3].2 as usize + index * 12;
        for (word, value) in [string(shorty), ty(result), type_list(params)]
            .into_iter()
            .enumerate()
        {
            write(&mut dex, at + word * 4, value);
        }
    }
    for (index, &(class, name, kind)) in fields.iter().enumerate() {
        let at = sections[4].2 as usize + index * 8;
        write(&mut dex, at, ty(class) | ty(kind) << 16);
        write(&mut dex, at + 4, string(name));
    }
    for (index, &(class, name, proto)) in methods.iter().enumerate() {
        let at = sections[5].2 as usize + index * 8;
        write(
            &mut dex,
            at,
            ty(class) | (protos.binary_search(&proto).unwrap() as u32) << 16,
        );
        write(&mut dex, at + 4, string(name));
    }

    dex.resize(dex.len().next_multiple_of(4), 0);
    sections.push((0x2001, 2 * classes.len() as u32, dex.len() as u32));
    let constructors: Vec<_> = classes
        .iter()
        .map(|class| {
            // <clinit> loads our JNI library. <init> calls the framework constructor.
            let load = if class.name == AUDIO {
                code(
                    &mut dex,
                    0,
                    &[
                        0x001a,
                        string(AUDIO_LIB) as u16,
                        0x1071,
                        method(property) as u16,
                        0,
                        0x000c,
                        0x1071,
                        method(absolute) as u16,
                        0,
                        0x000e,
                    ],
                )
            } else {
                code(
                    &mut dex,
                    0,
                    &[
                        0x001a,
                        string(library) as u16,
                        0x1071,
                        method(load) as u16,
                        0,
                        0x000e,
                    ],
                )
            };
            let registers = 1 + class.constructor_args.len() as u32;
            // Retain the original bytecode of the existing Home and Unicode IME APKs.
            // A range invokes the keyboard connection's three-register superclass constructor.
            let (invoke, operands) = match class.constructor_args {
                [] => (0x1070, 0),
                [_] => (0x2070, 0x10),
                args => {
                    assert!(args.iter().all(|ty| !matches!(*ty, "J" | "D")));
                    (0x76 | ((registers as u16) << 8), 0)
                }
            };
            let init = code(
                &mut dex,
                registers,
                &[
                    invoke,
                    method((class.parent, "<init>", class.constructor())) as u16,
                    operands,
                    0x000e,
                ],
            );
            (load, init)
        })
        .collect();
    sections.push((0x2000, classes.len() as u32, dex.len() as u32));
    for (index, (class, (load, init))) in classes.iter().zip(constructors).enumerate() {
        let at = sections[6].2 as usize + index * 32;
        let start = dex.len() as u32;
        for (word, value) in [
            ty(class.name),
            0x11,
            ty(class.parent),
            type_list(class.interfaces),
            u32::MAX,
            0,
            start,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write(&mut dex, at + word * 4, value);
        }
        let (statics, instances): (Vec<_>, Vec<_>) = class
            .fields
            .iter()
            .map(|&(name, kind, flags)| (field((class.name, name, kind)), flags))
            .partition(|entry| entry.1 & 8 != 0);
        let (mut direct, mut virtuals): (Vec<_>, Vec<_>) = class
            .natives
            .iter()
            .map(|&(name, proto, flags)| (method((class.name, name, proto)), flags | 0x100, 0))
            .partition(|entry| entry.1 & 8 != 0);
        direct.extend([
            (method((class.name, "<clinit>", VOID)), 0x10008, load),
            (
                method((class.name, "<init>", class.constructor())),
                0x10001,
                init,
            ),
        ]);
        for count in [statics.len(), instances.len(), direct.len(), virtuals.len()] {
            leb(&mut dex, count as u32);
        }
        for mut each in [statics, instances] {
            each.sort_unstable();
            let mut prev = 0;
            for (idx, flags) in each {
                leb(&mut dex, idx - prev);
                leb(&mut dex, flags);
                prev = idx;
            }
        }
        for each in [&mut direct, &mut virtuals] {
            each.sort_unstable();
            let mut prev = 0;
            for &(idx, flags, code) in each.iter() {
                leb(&mut dex, idx - prev);
                leb(&mut dex, flags);
                leb(&mut dex, code);
                prev = idx;
            }
        }
    }
    dex.resize(dex.len().next_multiple_of(4), 0);
    let map = dex.len() as u32;
    sections.push((0x1000, 1, map));
    sections.retain(|(_, count, _)| *count != 0);
    push(&mut dex, &[sections.len() as u32]);
    for (kind, count, offset) in sections {
        push(&mut dex, &[kind, count, offset]);
    }
    let size = dex.len() as u32;
    for (field, value) in [
        (32, size),
        (36, 112),
        (40, 0x12345678),
        (52, map),
        (104, size - data),
        (108, data),
    ] {
        write(&mut dex, field, value);
    }
    let signature = digest(&SHA1_FOR_LEGACY_USE_ONLY, &dex[32..]);
    dex[12..32].copy_from_slice(signature.as_ref());
    let (a, b) = dex[12..].iter().fold((1, 0), |(a, b), &byte| {
        let a = (a + u32::from(byte)) % 65521;
        (a, (a + b) % 65521)
    });
    write(&mut dex, 8, a | b << 16);
    dex
}
