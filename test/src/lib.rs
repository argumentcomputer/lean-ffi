//! FFI roundtrip functions for testing lean-ffi.
//!
//! Each function decodes a Lean value to a Rust representation using lean-ffi,
//! then re-encodes it back to a Lean value. The Lean test suite calls these via
//! `@[extern]` and checks that the round-tripped value equals the original.

use std::sync::LazyLock;

use lean_ffi::nat::Nat;
use lean_ffi::object::{
    ExternalClass, LeanArray, LeanBool, LeanByteArray, LeanCtor, LeanExcept, LeanExternal,
    LeanIOResult, LeanList, LeanNat, LeanObject, LeanOption, LeanProd, LeanString,
};

// =============================================================================
// Nat building
// =============================================================================

/// Build a Lean Nat from a Rust Nat.
fn build_nat(n: &Nat) -> LeanObject {
    if let Some(val) = n.to_u64() {
        if val <= (usize::MAX >> 1) as u64 {
            #[allow(clippy::cast_possible_truncation)]
            return LeanObject::box_usize(val as usize);
        }
        return LeanObject::from_nat_u64(val);
    }
    let bytes = n.to_le_bytes();
    let mut limbs: Vec<u64> = Vec::with_capacity(bytes.len().div_ceil(8));
    for chunk in bytes.chunks(8) {
        let mut arr = [0u8; 8];
        arr[..chunk.len()].copy_from_slice(chunk);
        limbs.push(u64::from_le_bytes(arr));
    }
    unsafe { lean_ffi::nat::lean_nat_from_limbs(limbs.len(), limbs.as_ptr()) }
}

// =============================================================================
// Roundtrip FFI functions
// =============================================================================

/// Round-trip a Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_nat(nat_ptr: LeanNat) -> LeanObject {
    let nat = Nat::from_obj(*nat_ptr);
    build_nat(&nat)
}

/// Round-trip a String: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_string(s_ptr: LeanString) -> LeanString {
    let s = s_ptr.to_string();
    LeanString::new(&s)
}

/// Round-trip a Bool: decode from Lean, re-encode.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_bool(bool_ptr: LeanBool) -> LeanBool {
    bool_ptr
}

/// Round-trip a List Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_list_nat(list_ptr: LeanList) -> LeanList {
    let nats: Vec<Nat> = list_ptr.collect(Nat::from_obj);
    let items: Vec<LeanObject> = nats.iter().map(build_nat).collect();
    items.into_iter().collect()
}

/// Round-trip an Array Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_array_nat(arr_ptr: LeanArray) -> LeanArray {
    let nats: Vec<Nat> = arr_ptr.map(Nat::from_obj);
    let arr = LeanArray::alloc(nats.len());
    for (i, nat) in nats.iter().enumerate() {
        arr.set(i, build_nat(nat));
    }
    arr
}

/// Round-trip a ByteArray: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_bytearray(ba: LeanByteArray) -> LeanByteArray {
    LeanByteArray::from_bytes(ba.as_bytes())
}

/// Round-trip an Option Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_option_nat(opt: LeanObject) -> LeanObject {
    if opt.is_scalar() {
        // none
        LeanOption::none().into()
    } else {
        // some val
        let nat = Nat::from_obj(opt.as_ctor().get(0));
        LeanOption::some(build_nat(&nat)).into()
    }
}

/// Round-trip a Point (structure with x, y : Nat).
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_point(point_ptr: LeanCtor) -> LeanObject {
    let x = Nat::from_obj(point_ptr.get(0));
    let y = Nat::from_obj(point_ptr.get(1));
    let point = LeanCtor::alloc(0, 2, 0);
    point.set(0, build_nat(&x));
    point.set(1, build_nat(&y));
    *point
}

/// Round-trip a NatTree (inductive: leaf Nat | node NatTree NatTree).
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_nat_tree(tree_ptr: LeanCtor) -> LeanObject {
    roundtrip_nat_tree_recursive(tree_ptr)
}

fn roundtrip_nat_tree_recursive(ctor: LeanCtor) -> LeanObject {
    match ctor.tag() {
        0 => {
            // leaf : Nat → NatTree
            let nat = Nat::from_obj(ctor.get(0));
            let leaf = LeanCtor::alloc(0, 1, 0);
            leaf.set(0, build_nat(&nat));
            *leaf
        }
        1 => {
            // node : NatTree → NatTree → NatTree
            let left = roundtrip_nat_tree_recursive(ctor.get(0).as_ctor());
            let right = roundtrip_nat_tree_recursive(ctor.get(1).as_ctor());
            let node = LeanCtor::alloc(1, 2, 0);
            node.set(0, left);
            node.set(1, right);
            *node
        }
        _ => panic!("Invalid NatTree tag: {}", ctor.tag()),
    }
}

// =============================================================================
// LeanProd roundtrip
// =============================================================================

/// Round-trip a Prod Nat Nat: decode fst/snd via LeanCtor, re-encode via LeanProd::new.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_prod_nat_nat(pair: LeanObject) -> LeanObject {
    let ctor = pair.as_ctor();
    let fst = Nat::from_obj(ctor.get(0));
    let snd = Nat::from_obj(ctor.get(1));
    LeanProd::new(build_nat(&fst), build_nat(&snd)).into()
}

// =============================================================================
// LeanExcept roundtrip
// =============================================================================

/// Round-trip an Except String Nat: decode ok/error, re-encode.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_except_string_nat(exc: LeanObject) -> LeanObject {
    let ctor = exc.as_ctor();
    match ctor.tag() {
        0 => {
            // Except.error (tag 0): field 0 is the error String
            let s = ctor.get(0).as_string();
            let msg = s.to_string();
            LeanExcept::error(LeanString::new(&msg)).into()
        }
        1 => {
            // Except.ok (tag 1): field 0 is the Nat value
            let nat = Nat::from_obj(ctor.get(0));
            LeanExcept::ok(build_nat(&nat)).into()
        }
        _ => panic!("Invalid Except tag: {}", ctor.tag()),
    }
}

/// Build an Except.error from a Rust string (tests LeanExcept::error_string).
#[unsafe(no_mangle)]
pub extern "C" fn rs_except_error_string(s: LeanString) -> LeanObject {
    let msg = s.to_string();
    LeanExcept::error_string(&msg).into()
}

// =============================================================================
// LeanIOResult roundtrip
// =============================================================================

/// Build a successful IO result wrapping a Nat (tests LeanIOResult::ok).
#[unsafe(no_mangle)]
pub extern "C" fn rs_io_result_ok_nat(nat_ptr: LeanNat) -> LeanObject {
    let nat = Nat::from_obj(*nat_ptr);
    LeanIOResult::ok(build_nat(&nat)).into()
}

/// Build an IO error from a string (tests LeanIOResult::error_string).
#[unsafe(no_mangle)]
pub extern "C" fn rs_io_result_error_string(s: LeanString) -> LeanObject {
    let msg = s.to_string();
    LeanIOResult::error_string(&msg).into()
}

// =============================================================================
// LeanCtor scalar fields
// =============================================================================

/// Round-trip a ScalarStruct (structure with obj : Nat, u8val : UInt8,
/// u32val : UInt32, u64val : UInt64).
/// Layout: tag 0, 1 obj field, 13 scalar bytes (1 + 4 + 8, padded).
#[unsafe(no_mangle)]
#[allow(deprecated)]
pub extern "C" fn rs_roundtrip_scalar_struct(ptr: LeanCtor) -> LeanObject {
    let obj_nat = Nat::from_obj(ptr.get(0));
    let u8val = ptr.scalar_u8(1, 0);
    let u32val = ptr.scalar_u32(1, 1);
    let u64val = ptr.scalar_u64(1, 5);

    let ctor = LeanCtor::alloc(0, 1, 13);
    ctor.set(0, build_nat(&obj_nat));
    ctor.set_scalar_u8(1, 0, u8val);
    ctor.set_scalar_u32(1, 1, u32val);
    ctor.set_scalar_u64(1, 5, u64val);
    *ctor
}

// =============================================================================
// box_u32 / box_u64 roundtrip
// =============================================================================

/// Round-trip a UInt32 (passed as raw uint32_t by Lean FFI).
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_uint32(val: u32) -> u32 {
    val
}

/// Round-trip a UInt64 (passed as raw uint64_t by Lean FFI).
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_uint64(val: u64) -> u64 {
    val
}

/// Round-trip an Array UInt32. Elements are boxed lean_object* inside the
/// array, so this exercises LeanObject::box_u32 / unbox_u32.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_array_uint32(arr_ptr: LeanArray) -> LeanArray {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_u32();
        new_arr.set(i, LeanObject::box_u32(val));
    }
    new_arr
}

/// Round-trip an Array UInt64. Elements are boxed lean_object* inside the
/// array, so this exercises LeanObject::box_u64 / unbox_u64.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_array_uint64(arr_ptr: LeanArray) -> LeanArray {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_u64();
        new_arr.set(i, LeanObject::box_u64(val));
    }
    new_arr
}

// =============================================================================
// LeanExternal<T> roundtrip
// =============================================================================

/// A simple Rust struct to store in a Lean external object.
#[derive(Debug, Clone, PartialEq)]
struct RustData {
    x: u64,
    y: u64,
    label: String,
}

static RUST_DATA_CLASS: LazyLock<ExternalClass> =
    LazyLock::new(ExternalClass::register_with_drop::<RustData>);

/// Create a LeanExternal<RustData> from three Lean values (x : UInt64, y : UInt64, label : String).
#[unsafe(no_mangle)]
pub extern "C" fn rs_external_create(x: u64, y: u64, label: LeanString) -> LeanObject {
    let data = RustData {
        x,
        y,
        label: label.to_string(),
    };
    let ext = LeanExternal::alloc(&RUST_DATA_CLASS, data);
    ext.into()
}

/// Read the x field from a LeanExternal<RustData>.
#[unsafe(no_mangle)]
pub extern "C" fn rs_external_get_x(obj: LeanObject) -> u64 {
    let ext = unsafe { LeanExternal::<RustData>::from_raw(obj.as_ptr()) };
    ext.get().x
}

/// Read the y field from a LeanExternal<RustData>.
#[unsafe(no_mangle)]
pub extern "C" fn rs_external_get_y(obj: LeanObject) -> u64 {
    let ext = unsafe { LeanExternal::<RustData>::from_raw(obj.as_ptr()) };
    ext.get().y
}

/// Read the label field from a LeanExternal<RustData>.
#[unsafe(no_mangle)]
pub extern "C" fn rs_external_get_label(obj: LeanObject) -> LeanString {
    let ext = unsafe { LeanExternal::<RustData>::from_raw(obj.as_ptr()) };
    LeanString::new(&ext.get().label)
}

// =============================================================================
// Extended scalar struct roundtrip (u8, u16, u32, u64, f64, f32)
// =============================================================================

/// Round-trip an ExtScalarStruct:
///   structure ExtScalarStruct where
///     obj : Nat; u8val : UInt8; u16val : UInt16
///     u32val : UInt32; u64val : UInt64; fval : Float; f32val : Float32
///
/// Lean sorts scalar fields by descending size:
///   u64val at 0, fval(f64) at 8, u32val at 16, f32val at 20, u16val at 24, u8val at 26
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_ext_scalar_struct(ptr: LeanCtor) -> LeanObject {
    let obj_nat = Nat::from_obj(ptr.get(0));
    // Read in Lean's packed order: 8B, 4B, 2B, 1B
    let u64val = ptr.scalar_u64(1, 0);
    let fval = ptr.scalar_f64(1, 8);
    let u32val = ptr.scalar_u32(1, 16);
    let f32val = ptr.scalar_f32(1, 20);
    let u16val = ptr.scalar_u16(1, 24);
    let u8val = ptr.scalar_u8(1, 26);

    // scalar_size: 8 + 8 + 4 + 4 + 2 + 1 = 27 bytes
    let ctor = LeanCtor::alloc(0, 1, 27);
    ctor.set(0, build_nat(&obj_nat));
    ctor.set_scalar_u64(1, 0, u64val);
    ctor.set_scalar_f64(1, 8, fval);
    ctor.set_scalar_u32(1, 16, u32val);
    ctor.set_scalar_f32(1, 20, f32val);
    ctor.set_scalar_u16(1, 24, u16val);
    ctor.set_scalar_u8(1, 26, u8val);
    *ctor
}

// =============================================================================
// USize struct roundtrip
// =============================================================================

/// Round-trip a USizeStruct:
///   structure USizeStruct where
///     obj : Nat; uval : USize; u8val : UInt8
///
/// Layout: 1 obj field, then USize (slot 0), then u8 at byte offset
/// past the usize slot.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_usize_struct(ptr: LeanCtor) -> LeanObject {
    let obj_nat = Nat::from_obj(ptr.get(0));
    let uval = ptr.scalar_usize(1, 0);
    // u8val is after the usize slot: 1 usize slot = 8 bytes on 64-bit
    let u8val = ptr.scalar_u8(1, 8);

    let ctor = LeanCtor::alloc(0, 1, 16); // 8 (usize) + 1 (u8) padded
    ctor.set(0, build_nat(&obj_nat));
    ctor.set_scalar_usize(1, 0, uval);
    ctor.set_scalar_u8(1, 8, u8val);
    *ctor
}

// =============================================================================
// Float / Float32 / USize scalar roundtrips
// =============================================================================

/// Round-trip a Float (f64) — passed as raw scalar across FFI.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_float(val: f64) -> f64 {
    val
}

/// Round-trip a Float32 (f32) — passed as raw scalar across FFI.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_float32(val: f32) -> f32 {
    val
}

/// Round-trip a USize — passed as raw scalar across FFI.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_usize(val: usize) -> usize {
    val
}

/// Round-trip an Array Float. Elements are boxed f64 inside the array.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_array_float(arr_ptr: LeanArray) -> LeanArray {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_f64();
        new_arr.set(i, LeanObject::box_f64(val));
    }
    new_arr
}

/// Round-trip an Array Float32. Elements are boxed f32 inside the array.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_array_float32(arr_ptr: LeanArray) -> LeanArray {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_f32();
        new_arr.set(i, LeanObject::box_f32(val));
    }
    new_arr
}

// =============================================================================
// LeanString::from_bytes roundtrip
// =============================================================================

/// Round-trip a String using LeanString::from_bytes instead of LeanString::new.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_string_from_bytes(s_ptr: LeanString) -> LeanString {
    let s = s_ptr.to_string();
    LeanString::from_bytes(s.as_bytes())
}

// =============================================================================
// LeanArray::push roundtrip
// =============================================================================

/// Round-trip an Array Nat by pushing each element into a new array.
#[unsafe(no_mangle)]
pub extern "C" fn rs_roundtrip_array_push(arr_ptr: LeanArray) -> LeanArray {
    let nats: Vec<Nat> = arr_ptr.map(Nat::from_obj);
    let mut arr = LeanArray::alloc(0);
    for nat in &nats {
        arr = arr.push(build_nat(nat));
    }
    arr
}
