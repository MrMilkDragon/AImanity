use core::ffi::{CStr, c_char, c_void};
use core::ptr::null;

pub type Obj = *mut c_void;
pub type RawEnv = *const *const *const c_void;

#[repr(C)]
pub union Arg {
    object: Obj,
    integer: i32,
    long: i64,
    float: f32,
}

macro_rules! argument {
    ($ty:ty, $field:ident) => {
        impl From<$ty> for Arg {
            fn from(value: $ty) -> Self {
                Self { $field: value }
            }
        }
    };
}
argument!(Obj, object);
argument!(i32, integer);
argument!(i64, long);
argument!(f32, float);

macro_rules! args {
    ($($item:expr),* $(,)?) => { &[$(Arg::from($item)),*] };
}
pub(super) use args;

macro_rules! call {
    ($env:expr, $slot:literal, ($($types:ty),*) -> $out:ty, $($inputs:expr),*) => {{
        let function: unsafe extern "system" fn(RawEnv, $($types),*) -> $out =
            unsafe { core::mem::transmute(*(*$env.0).add($slot)) };
        unsafe { function($env.0, $($inputs),*) }
    }};
}

#[derive(Clone, Copy)]
pub struct Env(pub RawEnv);

macro_rules! instance_calls {
    ($(($name:ident, $ty:ty, $slot:literal)),* $(,)?) => {$ (
        pub fn $name(self, target: Obj, name: &CStr, signature: &CStr, args: &[Arg]) -> $ty {
            let class = call!(self, 31, (Obj) -> Obj, target);
            let method = self.method(class, name, signature);
            self.release(class);
            call!(self, $slot, (Obj, Obj, *const Arg) -> $ty, target, method, args.as_ptr())
        }
    )*};
}

macro_rules! static_calls {
    ($(($name:ident, $ty:ty, $slot:literal)),* $(,)?) => {$ (
        pub fn $name(self, class: &CStr, name: &CStr, signature: &CStr, args: &[Arg]) -> $ty {
            let class = self.class(class);
            let method = call!(self, 113, (Obj, *const c_char, *const c_char) -> Obj,
                class, name.as_ptr(), signature.as_ptr());
            let result = call!(self, $slot, (Obj, Obj, *const Arg) -> $ty, class, method, args.as_ptr());
            self.release(class);
            result
        }
    )*};
}

macro_rules! fields {
    ($(($getter:ident, $setter:ident, $ty:ty, $get:literal, $set:literal, $signature:expr)),* $(,)?) => {$ (
        pub fn $getter(self, target: Obj, name: &CStr) -> $ty {
            let field = self.field(target, name, $signature);
            call!(self, $get, (Obj, Obj) -> $ty, target, field)
        }
        pub fn $setter(self, target: Obj, name: &CStr, value: $ty) {
            let field = self.field(target, name, $signature);
            call!(self, $set, (Obj, Obj, $ty) -> (), target, field, value)
        }
    )*};
}

impl Env {
    instance_calls!(
        (obj, Obj, 36),
        (int, i32, 51),
        (float, f32, 57),
        (boolean, u8, 39),
        (void, (), 63)
    );
    static_calls!((static_obj, Obj, 116), (static_int, i32, 131));
    fields!(
        (get_int, set_int, i32, 100, 109, c"I"),
        (get_bool, set_bool, u8, 96, 105, c"Z"),
        (get_float, set_float, f32, 102, 111, c"F")
    );

    pub fn class(self, name: &CStr) -> Obj {
        call!(self, 6, (*const c_char) -> Obj, name.as_ptr())
    }

    fn method(self, class: Obj, name: &CStr, signature: &CStr) -> Obj {
        call!(self, 33, (Obj, *const c_char, *const c_char) -> Obj, class, name.as_ptr(), signature.as_ptr())
    }

    fn field(self, target: Obj, name: &CStr, signature: &CStr) -> Obj {
        let class = call!(self, 31, (Obj) -> Obj, target);
        let field = call!(self, 94, (Obj, *const c_char, *const c_char) -> Obj, class, name.as_ptr(), signature.as_ptr());
        self.release(class);
        field
    }

    pub fn get(self, target: Obj, name: &CStr, signature: &CStr) -> Obj {
        let field = self.field(target, name, signature);
        call!(self, 95, (Obj, Obj) -> Obj, target, field)
    }

    pub fn set(self, target: Obj, name: &CStr, signature: &CStr, value: Obj) {
        let field = self.field(target, name, signature);
        call!(self, 104, (Obj, Obj, Obj) -> (), target, field, value);
    }

    pub fn static_field(self, class: &CStr, name: &CStr, signature: &CStr) -> Obj {
        let class = self.class(class);
        let field = call!(self, 144, (Obj, *const c_char, *const c_char) -> Obj, class, name.as_ptr(), signature.as_ptr());
        let value = call!(self, 145, (Obj, Obj) -> Obj, class, field);
        self.release(class);
        value
    }

    pub fn new(self, name: &CStr, signature: &CStr, args: &[Arg]) -> Obj {
        let class = self.class(name);
        let constructor = self.method(class, c"<init>", signature);
        let object =
            call!(self, 30, (Obj, Obj, *const Arg) -> Obj, class, constructor, args.as_ptr());
        self.release(class);
        object
    }

    pub fn string(self, value: &CStr) -> Obj {
        call!(self, 167, (*const c_char) -> Obj, value.as_ptr())
    }

    pub fn super_void(
        self,
        target: Obj,
        class: &CStr,
        name: &CStr,
        signature: &CStr,
        args: &[Arg],
    ) {
        let class = self.class(class);
        let method = self.method(class, name, signature);
        call!(self, 93, (Obj, Obj, Obj, *const Arg) -> (), target, class, method, args.as_ptr());
        self.release(class);
    }

    pub fn super_bool(
        self,
        target: Obj,
        class: &CStr,
        name: &CStr,
        signature: &CStr,
        args: &[Arg],
    ) -> u8 {
        let class = self.class(class);
        let method = self.method(class, name, signature);
        let result = call!(self, 69, (Obj, Obj, Obj, *const Arg) -> u8, target, class, method, args.as_ptr());
        self.release(class);
        result
    }

    pub fn ints(self, values: &[i32]) -> Obj {
        let len = values.len() as i32;
        let array = call!(self, 179, (i32) -> Obj, len);
        call!(self, 211, (Obj, i32, i32, *const i32) -> (), array, 0, len, values.as_ptr());
        array
    }

    pub fn array_size(self, array: Obj) -> i32 {
        call!(self, 171, (Obj) -> i32, array)
    }

    pub fn array_at(self, array: Obj, index: i32) -> Obj {
        call!(self, 173, (Obj, i32) -> Obj, array, index)
    }

    pub fn instance_of(self, object: Obj, class: &CStr) -> bool {
        if object.is_null() {
            return false;
        }
        let class = self.class(class);
        let result = call!(self, 32, (Obj, Obj) -> u8, object, class);
        self.release(class);
        result != 0
    }

    pub fn release(self, object: Obj) {
        call!(self, 23, (Obj) -> (), object);
    }

    pub fn exception(self) -> bool {
        let thrown = call!(self, 15, () -> Obj, );
        if thrown.is_null() {
            return false;
        }
        call!(self, 17, () -> (), );
        self.release(thrown);
        true
    }

    pub fn push(self) -> bool {
        call!(self, 19, (i32) -> i32, 48) == 0
    }

    pub fn pop(self) {
        call!(self, 20, (Obj) -> Obj, null::<c_void>().cast_mut());
    }
}
