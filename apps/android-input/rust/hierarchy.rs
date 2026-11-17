use super::{Arg, Env, Object, RawEnv};
use core::{
    ffi::{CStr, c_char},
    ptr::null_mut,
};

const STRING: &CStr = c"()Ljava/lang/String;";
const SEQUENCE: &CStr = c"()Ljava/lang/CharSequence;";
const SERIALIZER: &CStr = c"(Ljava/lang/String;Ljava/lang/String;)Lorg/xmlpull/v1/XmlSerializer;";

fn object(value: Object) -> Arg {
    Arg { object: value }
}

impl Env {
    fn boolean(self, target: Object, name: &CStr) -> bool {
        let method = self.instance_method(target, name, c"()Z");
        jni!(self, 39, (Object, Object, *const Arg) -> u8,
            target, method, core::ptr::null())
            != 0
    }
}

pub(super) fn capture(env: Env, root: Object) -> Option<Object> {
    let serializer = env.static_object(
        c"android/util/Xml",
        c"newSerializer",
        c"()Lorg/xmlpull/v1/XmlSerializer;",
        &[],
    );
    let writer = env.create(c"java/io/StringWriter", c"()V", &[]);
    env.void(
        serializer,
        c"setOutput",
        c"(Ljava/io/Writer;)V",
        &[object(writer)],
    );
    let boolean = env.static_object(
        c"java/lang/Boolean",
        c"valueOf",
        c"(Z)Ljava/lang/Boolean;",
        &[Arg { integer: 1 }],
    );
    env.void(
        serializer,
        c"startDocument",
        c"(Ljava/lang/String;Ljava/lang/Boolean;)V",
        &[object(env.string(c"UTF-8")), object(boolean)],
    );

    let rect = env.create(c"android/graphics/Rect", c"()V", &[]);
    let class = env.class(c"android/graphics/Rect");
    let fields = [c"left", c"top", c"right", c"bottom"].map(|field| {
        jni!(env, 94, (Object, *const c_char, *const c_char) -> Object,
            class, field.as_ptr(), c"I".as_ptr())
    });
    env.release(class);
    let class = env.class(c"org/xmlpull/v1/XmlSerializer");
    let start = env.method(class, c"startTag", SERIALIZER);
    let end = env.method(class, c"endTag", SERIALIZER);
    let attribute = env.method(
        class,
        c"attribute",
        c"(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Lorg/xmlpull/v1/XmlSerializer;",
    );
    env.release(class);
    let mut tree = Tree {
        env,
        serializer,
        start,
        end,
        attribute,
        rect,
        fields,
        remaining: 1500,
        node: env.string(c"node"),
        empty: env.string(c""),
        yes: env.string(c"true"),
        no: env.string(c"false"),
    };
    let hierarchy = env.string(c"hierarchy");
    tree.tag(start, hierarchy)?;
    tree.append(root, 0)?;
    tree.tag(end, hierarchy)?;
    env.void(serializer, c"endDocument", c"()V", &[]);
    if env.discard_exception() {
        return None;
    }
    Some(env.object(writer, c"toString", STRING, &[]))
}

struct Tree {
    env: Env,
    serializer: Object,
    start: Object,
    end: Object,
    attribute: Object,
    rect: Object,
    fields: [Object; 4],
    remaining: u32,
    node: Object,
    empty: Object,
    yes: Object,
    no: Object,
}

impl Tree {
    fn tag(&self, method: Object, name: Object) -> Option<()> {
        let result =
            self.env
                .call_object(self.serializer, method, &[object(null_mut()), object(name)]);
        self.env.release(result);
        (!result.is_null()).then_some(())
    }

    fn attr(&self, name: &CStr, value: Object) -> Option<()> {
        let key = self.env.string(name);
        let result = self.env.call_object(
            self.serializer,
            self.attribute,
            &[object(null_mut()), object(key), object(value)],
        );
        self.env.release(key);
        self.env.release(result);
        (!result.is_null()).then_some(())
    }

    fn text(&self, node: Object, name: &CStr, method: &CStr, signature: &CStr) -> Option<()> {
        let env = self.env;
        let sequence = env.object(node, method, signature, &[]);
        if sequence.is_null() {
            return self.attr(name, self.empty);
        }
        let text = env.object(sequence, c"toString", STRING, &[]);
        let truncated = if jni!(env, 164, (Object) -> i32, text) > 512 {
            env.object(
                text,
                c"substring",
                c"(II)Ljava/lang/String;",
                &[Arg { integer: 0 }, Arg { integer: 512 }],
            )
        } else {
            text
        };
        let result = self.attr(name, truncated);
        if truncated != text {
            env.release(truncated);
        }
        env.release(text);
        env.release(sequence);
        result
    }

    fn append(&mut self, node: Object, depth: u8) -> Option<()> {
        if self.remaining == 0 || depth > 48 {
            return None;
        }
        self.remaining -= 1;
        let env = self.env;
        self.tag(self.start, self.node)?;
        self.text(node, c"package", c"getPackageName", SEQUENCE)?;
        self.text(node, c"class", c"getClassName", SEQUENCE)?;
        self.text(node, c"resource-id", c"getViewIdResourceName", STRING)?;
        let password = env.boolean(node, c"isPassword");
        if password {
            self.attr(c"text", self.empty)?;
            self.attr(c"content-desc", self.empty)?;
        } else {
            self.text(node, c"text", c"getText", SEQUENCE)?;
            self.text(node, c"content-desc", c"getContentDescription", SEQUENCE)?;
        }
        for (name, getter) in [
            (c"enabled", c"isEnabled"),
            (c"visible-to-user", c"isVisibleToUser"),
            (c"clickable", c"isClickable"),
            (c"scrollable", c"isScrollable"),
            (c"editable", c"isEditable"),
        ] {
            self.attr(
                name,
                if env.boolean(node, getter) {
                    self.yes
                } else {
                    self.no
                },
            )?;
        }
        self.attr(c"password", if password { self.yes } else { self.no })?;
        env.void(
            node,
            c"getBoundsInScreen",
            c"(Landroid/graphics/Rect;)V",
            &[object(self.rect)],
        );
        let [left, top, right, bottom] = self
            .fields
            .map(|field| jni!(env, 100, (Object, Object) -> i32, self.rect, field));
        let mut bounds = Bounds([0; 52], 0);
        for (x, y) in [(left, top), (right, bottom)] {
            bounds.push(b'[');
            bounds.number(x);
            bounds.push(b',');
            bounds.number(y);
            bounds.push(b']');
        }
        let text = jni!(env, 167, (*const c_char) -> Object, bounds.0.as_ptr().cast());
        let added = self.attr(c"bounds", text);
        env.release(text);
        added?;

        for integer in 0..env.integer(node, c"getChildCount") {
            let child = env.object(
                node,
                c"getChild",
                c"(I)Landroid/view/accessibility/AccessibilityNodeInfo;",
                &[Arg { integer }],
            );
            if child.is_null() {
                continue;
            }
            let added = self.append(child, depth + 1);
            if added.is_none() {
                env.discard_exception();
            }
            env.void(child, c"recycle", c"()V", &[]);
            env.release(child);
            added?;
        }
        self.tag(self.end, self.node)
    }
}

struct Bounds([u8; 52], usize);

impl Bounds {
    fn push(&mut self, byte: u8) {
        // Four signed i32 coordinates and six delimiters occupy at most 50 bytes.
        unsafe { self.0.as_mut_ptr().add(self.1).write(byte) };
        self.1 += 1;
    }

    fn number(&mut self, value: i32) {
        if value < 0 {
            self.push(b'-');
        }
        let mut value = value.unsigned_abs();
        let mut divisor = 1u32;
        while value / 10 >= divisor {
            divisor *= 10;
        }
        while let Some(place) = core::num::NonZeroU32::new(divisor) {
            self.push(b'0' + (value / place.get()) as u8);
            value %= place.get();
            divisor /= 10;
        }
    }
}
