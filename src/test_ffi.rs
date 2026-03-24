//! FFI roundtrip functions for testing lean-ffi.
//!
//! Each function decodes a Lean value to a Rust representation using lean-ffi,
//! then re-encodes it back to a Lean value. The Lean test suite calls these via
//! `@[extern]` and checks that the round-tripped value equals the original.
//!
//! All parameters use `@&` (borrowed) in the Lean declarations, so the Rust
//! side receives `LeanBorrowed<'_>` — no `lean_dec` on inputs.

use std::sync::LazyLock;

use crate::include;
use crate::nat::Nat;
use crate::object::{
    ExternalClass, LeanArray, LeanBool, LeanBorrowed, LeanByteArray, LeanCtor, LeanExcept,
    LeanExternal, LeanIOResult, LeanList, LeanNat, LeanOption, LeanOwned, LeanProd, LeanRef,
    LeanString,
};

// =============================================================================
// Domain types for Lean structures
// =============================================================================

crate::lean_domain_type! {
    /// Lean `Point` — structure Point where x : Nat; y : Nat
    LeanPoint;
    /// Lean `NatTree` — inductive NatTree | leaf : Nat → NatTree | node : NatTree → NatTree → NatTree
    LeanNatTree;
    /// Lean `ScalarStruct` — structure ScalarStruct where obj : Nat; u8val : UInt8; u32val : UInt32; u64val : UInt64
    LeanScalarStruct;
    /// Lean `ExtScalarStruct` — all scalar types
    LeanExtScalarStruct;
    /// Lean `USizeStruct` — structure USizeStruct where obj : Nat; uval : USize; u8val : UInt8
    LeanUSizeStruct;
    /// Lean `RustData` — opaque external object
    LeanRustData;
}

/// Build a Lean Nat from a Rust Nat (delegates to `Nat::to_lean`).
fn build_nat(n: &Nat) -> LeanOwned {
    n.to_lean().into()
}

// =============================================================================
// Roundtrip FFI functions
// =============================================================================

/// Round-trip a Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_nat(
    nat_ptr: LeanNat<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let nat = Nat::from_obj(nat_ptr.inner());
    nat.to_lean()
}

/// Round-trip a String: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_string(
    s_ptr: LeanString<LeanBorrowed<'_>>,
) -> LeanString<LeanOwned> {
    let s = s_ptr.to_string();
    LeanString::new(&s)
}

/// Round-trip a Bool: decode from Lean, re-encode.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_bool(
    bool_ptr: LeanBool<LeanBorrowed<'_>>,
) -> LeanBool<LeanOwned> {
    let val = bool_ptr.to_bool();
    if val {
        LeanBool::new(LeanOwned::from_enum_tag(1))
    } else {
        LeanBool::new(LeanOwned::from_enum_tag(0))
    }
}

/// Round-trip a List Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_list_nat(
    list_ptr: LeanList<LeanBorrowed<'_>>,
) -> LeanList<LeanOwned> {
    let nats: Vec<Nat> = list_ptr.collect(|b| Nat::from_obj(&b));
    let items: Vec<LeanOwned> = nats.iter().map(build_nat).collect();
    items.into_iter().collect()
}

/// Round-trip an Array Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_array_nat(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let nats: Vec<Nat> = arr_ptr.map(|b| Nat::from_obj(&b));
    let arr = LeanArray::alloc(nats.len());
    for (i, nat) in nats.iter().enumerate() {
        arr.set(i, build_nat(nat));
    }
    arr
}

/// Round-trip a ByteArray: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_bytearray(
    ba: LeanByteArray<LeanBorrowed<'_>>,
) -> LeanByteArray<LeanOwned> {
    LeanByteArray::from_bytes(ba.as_bytes())
}

/// Round-trip an Option Nat: decode from Lean, re-encode to Lean.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_option_nat(
    opt: LeanOption<LeanBorrowed<'_>>,
) -> LeanOption<LeanOwned> {
    if opt.inner().is_scalar() {
        LeanOption::none()
    } else {
        let ctor = opt.as_ctor();
        let nat = Nat::from_obj(&ctor.get(0));
        LeanOption::some(build_nat(&nat))
    }
}

/// Round-trip a Point (structure with x, y : Nat).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_point(
    point_ptr: LeanPoint<LeanBorrowed<'_>>,
) -> LeanPoint<LeanOwned> {
    let ctor = point_ptr.as_ctor();
    let x = Nat::from_obj(&ctor.get(0));
    let y = Nat::from_obj(&ctor.get(1));
    let out = LeanCtor::alloc(0, 2, 0);
    out.set(0, build_nat(&x));
    out.set(1, build_nat(&y));
    LeanPoint::new(out.into())
}

/// Round-trip a NatTree (inductive: leaf Nat | node NatTree NatTree).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_nat_tree(
    tree_ptr: LeanNatTree<LeanBorrowed<'_>>,
) -> LeanNatTree<LeanOwned> {
    LeanNatTree::new(roundtrip_nat_tree_recursive(&tree_ptr.as_ctor()))
}

fn roundtrip_nat_tree_recursive(ctor: &LeanCtor<impl LeanRef>) -> LeanOwned {
    match ctor.tag() {
        0 => {
            // leaf : Nat → NatTree
            let nat = Nat::from_obj(&ctor.get(0));
            let leaf = LeanCtor::alloc(0, 1, 0);
            leaf.set(0, build_nat(&nat));
            leaf.into()
        }
        1 => {
            // node : NatTree → NatTree → NatTree
            let left = roundtrip_nat_tree_recursive(&ctor.get(0).as_ctor());
            let right = roundtrip_nat_tree_recursive(&ctor.get(1).as_ctor());
            let node = LeanCtor::alloc(1, 2, 0);
            node.set(0, left);
            node.set(1, right);
            node.into()
        }
        _ => panic!("Invalid NatTree tag: {}", ctor.tag()),
    }
}

// =============================================================================
// LeanProd roundtrip
// =============================================================================

/// Round-trip a Prod Nat Nat: decode fst/snd, re-encode via LeanProd::new.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_prod_nat_nat(
    pair: LeanProd<LeanBorrowed<'_>>,
) -> LeanProd<LeanOwned> {
    let fst = Nat::from_obj(&pair.fst());
    let snd = Nat::from_obj(&pair.snd());
    LeanProd::new(build_nat(&fst), build_nat(&snd))
}

// =============================================================================
// LeanExcept roundtrip
// =============================================================================

/// Round-trip an Except String Nat: decode ok/error, re-encode.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_except_string_nat(
    exc: LeanExcept<LeanBorrowed<'_>>,
) -> LeanExcept<LeanOwned> {
    match exc.into_result() {
        Err(err) => {
            let s = err.as_string();
            LeanExcept::error(LeanString::new(&s.to_string()))
        }
        Ok(val) => {
            let nat = Nat::from_obj(&val);
            LeanExcept::ok(build_nat(&nat))
        }
    }
}

/// Build an Except.error from a Rust string (tests LeanExcept::error_string).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_except_error_string(
    s: LeanString<LeanBorrowed<'_>>,
) -> LeanExcept<LeanOwned> {
    LeanExcept::error_string(&s.to_string())
}

// =============================================================================
// LeanIOResult roundtrip
// =============================================================================

/// Build a successful IO result wrapping a Nat (tests LeanIOResult::ok).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_io_result_ok_nat(
    nat_ptr: LeanNat<LeanBorrowed<'_>>,
) -> LeanIOResult<LeanOwned> {
    let nat = Nat::from_obj(nat_ptr.inner());
    LeanIOResult::ok(build_nat(&nat))
}

/// Build an IO error from a string (tests LeanIOResult::error_string).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_io_result_error_string(
    s: LeanString<LeanBorrowed<'_>>,
) -> LeanIOResult<LeanOwned> {
    LeanIOResult::error_string(&s.to_string())
}

// =============================================================================
// LeanCtor scalar fields
// =============================================================================

/// Round-trip a ScalarStruct.
/// Lean layout: 1 obj field, then scalars by descending size: u64(0), u32(8), u8(12).
/// Total scalar size: 8 + 4 + 1 = 13 bytes.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_scalar_struct(
    ptr: LeanScalarStruct<LeanBorrowed<'_>>,
) -> LeanScalarStruct<LeanOwned> {
    let ctor = ptr.as_ctor();
    let obj_nat = Nat::from_obj(&ctor.get(0));
    let u64val = ctor.get_u64(1, 0);
    let u32val = ctor.get_u32(1, 8);
    let u8val = ctor.get_u8(1, 12);

    let out = LeanCtor::alloc(0, 1, 13);
    out.set(0, build_nat(&obj_nat));
    out.set_u64(1, 0, u64val);
    out.set_u32(1, 8, u32val);
    out.set_u8(1, 12, u8val);
    LeanScalarStruct::new(out.into())
}

// =============================================================================
// box_u32 / box_u64 roundtrip
// =============================================================================

/// Round-trip a UInt32 (passed as raw uint32_t by Lean FFI).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_uint32(val: u32) -> u32 {
    val
}

/// Round-trip a UInt64 (passed as raw uint64_t by Lean FFI).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_uint64(val: u64) -> u64 {
    val
}

/// Round-trip an Array UInt32.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_array_uint32(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_u32();
        new_arr.set(i, LeanOwned::box_u32(val));
    }
    new_arr
}

/// Round-trip an Array UInt64.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_array_uint64(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_u64();
        new_arr.set(i, LeanOwned::box_u64(val));
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

/// Create a LeanExternal<RustData> from three Lean values.
/// Note: label is @& (borrowed), x/y are scalar UInt64.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_create(
    x: u64,
    y: u64,
    label: LeanString<LeanBorrowed<'_>>,
) -> LeanRustData<LeanOwned> {
    let data = RustData {
        x,
        y,
        label: label.to_string(),
    };
    let ext = LeanExternal::alloc(&RUST_DATA_CLASS, data);
    LeanRustData::new(ext.into())
}

/// Read the x field from a LeanExternal<RustData> (@& borrowed).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_get_x(obj: LeanRustData<LeanBorrowed<'_>>) -> u64 {
    let ext =
        unsafe { LeanExternal::<RustData, LeanBorrowed<'_>>::from_raw_borrowed(obj.as_raw()) };
    ext.get().x
}

/// Read the y field from a LeanExternal<RustData> (@& borrowed).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_get_y(obj: LeanRustData<LeanBorrowed<'_>>) -> u64 {
    let ext =
        unsafe { LeanExternal::<RustData, LeanBorrowed<'_>>::from_raw_borrowed(obj.as_raw()) };
    ext.get().y
}

/// Read the label field from a LeanExternal<RustData> (@& borrowed).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_get_label(
    obj: LeanRustData<LeanBorrowed<'_>>,
) -> LeanString<LeanOwned> {
    let ext =
        unsafe { LeanExternal::<RustData, LeanBorrowed<'_>>::from_raw_borrowed(obj.as_raw()) };
    LeanString::new(&ext.get().label)
}

// =============================================================================
// Extended scalar struct roundtrip (u8, u16, u32, u64, f64, f32)
// =============================================================================

/// Round-trip an ExtScalarStruct.
/// Lean layout: 1 obj, then descending size: u64(0), f64(8), u32(16), f32(20), u16(24), u8(26).
/// Total scalar: 27 bytes.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_ext_scalar_struct(
    ptr: LeanExtScalarStruct<LeanBorrowed<'_>>,
) -> LeanExtScalarStruct<LeanOwned> {
    let ctor = ptr.as_ctor();
    let obj_nat = Nat::from_obj(&ctor.get(0));
    let u64val = ctor.get_u64(1, 0);
    let fval = ctor.get_f64(1, 8);
    let u32val = ctor.get_u32(1, 16);
    let f32val = ctor.get_f32(1, 20);
    let u16val = ctor.get_u16(1, 24);
    let u8val = ctor.get_u8(1, 26);

    let out = LeanCtor::alloc(0, 1, 27);
    out.set(0, build_nat(&obj_nat));
    out.set_u64(1, 0, u64val);
    out.set_f64(1, 8, fval);
    out.set_u32(1, 16, u32val);
    out.set_f32(1, 20, f32val);
    out.set_u16(1, 24, u16val);
    out.set_u8(1, 26, u8val);
    LeanExtScalarStruct::new(out.into())
}

// =============================================================================
// USize struct roundtrip
// =============================================================================

/// Round-trip a USizeStruct.
/// Lean layout: 1 obj field, then usize (slot 0), then u8 at scalar offset 0.
/// Alloc: num_objs=1, scalar_sz=9 (8 for usize slot + 1 for u8).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_usize_struct(
    ptr: LeanUSizeStruct<LeanBorrowed<'_>>,
) -> LeanUSizeStruct<LeanOwned> {
    let ctor = ptr.as_ctor();
    let obj_nat = Nat::from_obj(&ctor.get(0));
    let uval = ctor.get_usize(1, 0);
    let u8val = ctor.get_u8(2, 0);

    let out = LeanCtor::alloc(0, 1, 9);
    out.set(0, build_nat(&obj_nat));
    out.set_usize(1, 0, uval);
    out.set_u8(2, 0, u8val);
    LeanUSizeStruct::new(out.into())
}

// =============================================================================
// Float / Float32 / USize scalar roundtrips
// =============================================================================

/// Round-trip a Float (f64) — passed as raw scalar across FFI.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_float(val: f64) -> f64 {
    val
}

/// Round-trip a Float32 (f32) — passed as raw scalar across FFI.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_float32(val: f32) -> f32 {
    val
}

/// Round-trip a USize — passed as raw scalar across FFI.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_usize(val: usize) -> usize {
    val
}

/// Round-trip an Array Float.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_array_float(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_f64();
        new_arr.set(i, LeanOwned::box_f64(val));
    }
    new_arr
}

/// Round-trip an Array Float32.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_array_float32(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let len = arr_ptr.len();
    let new_arr = LeanArray::alloc(len);
    for i in 0..len {
        let val = arr_ptr.get(i).unbox_f32();
        new_arr.set(i, LeanOwned::box_f32(val));
    }
    new_arr
}

// =============================================================================
// LeanString::from_bytes roundtrip
// =============================================================================

/// Round-trip a String using LeanString::from_bytes instead of LeanString::new.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_string_from_bytes(
    s_ptr: LeanString<LeanBorrowed<'_>>,
) -> LeanString<LeanOwned> {
    let s = s_ptr.to_string();
    LeanString::from_bytes(s.as_bytes())
}

// =============================================================================
// LeanArray::push roundtrip
// =============================================================================

/// Round-trip an Array Nat by pushing each element into a new array.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_array_push(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let nats: Vec<Nat> = arr_ptr.map(|b| Nat::from_obj(&b));
    let mut arr = LeanArray::alloc(0);
    for nat in &nats {
        arr = arr.push(build_nat(nat));
    }
    arr
}

// =============================================================================
// Owned argument tests (NO @& — Lean transfers ownership, Rust must lean_dec)
// =============================================================================

/// Round-trip a Nat with owned arg (no @&). Tests that LeanOwned Drop correctly
/// calls lean_dec on the input without double-free or leak.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_nat_roundtrip(nat_ptr: LeanNat<LeanOwned>) -> LeanNat<LeanOwned> {
    let nat = Nat::from_obj(nat_ptr.inner());
    nat.to_lean()
    // nat_ptr drops here → lean_dec (correct for owned arg)
}

/// Round-trip a String with owned arg. Tests LeanOwned Drop on strings.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_string_roundtrip(
    s_ptr: LeanString<LeanOwned>,
) -> LeanString<LeanOwned> {
    let s = s_ptr.to_string();
    LeanString::new(&s)
    // s_ptr drops here → lean_dec
}

/// Round-trip an Array Nat with owned arg. Tests LeanOwned Drop on arrays.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_array_nat_roundtrip(
    arr_ptr: LeanArray<LeanOwned>,
) -> LeanArray<LeanOwned> {
    let nats: Vec<Nat> = arr_ptr.map(|b| Nat::from_obj(&b));
    let arr = LeanArray::alloc(nats.len());
    for (i, nat) in nats.iter().enumerate() {
        arr.set(i, build_nat(nat));
    }
    arr
    // arr_ptr drops here → lean_dec
}

/// Round-trip a List Nat with owned arg. Tests LeanOwned Drop on lists.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_list_nat_roundtrip(
    list_ptr: LeanList<LeanOwned>,
) -> LeanList<LeanOwned> {
    let nats: Vec<Nat> = list_ptr.collect(|b| Nat::from_obj(&b));
    let items: Vec<LeanOwned> = nats.iter().map(build_nat).collect();
    items.into_iter().collect()
    // list_ptr drops here → lean_dec
}

/// Two owned args: take an array and a nat (both owned), append nat to array.
/// Tests Drop on two owned args simultaneously.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_append_nat(
    arr: LeanArray<LeanOwned>,
    nat: LeanNat<LeanOwned>,
) -> LeanArray<LeanOwned> {
    let n = Nat::from_obj(nat.inner());
    // arr is consumed by push (ownership transferred to lean_array_push)
    arr.push(build_nat(&n))
    // nat drops here → lean_dec
}

/// Owned arg that we explicitly drop early (by letting it go out of scope)
/// then return a completely new value. Tests that Drop runs at the right time.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_drop_and_replace(
    s: LeanString<LeanOwned>,
) -> LeanString<LeanOwned> {
    let len = s.byte_len();
    drop(s); // explicit early drop → lean_dec
    LeanString::new(&format!("replaced:{len}"))
}

/// Three owned args: merge three lists into one.
/// Tests Drop on multiple owned args with complex ownership flow.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_merge_lists(
    a: LeanList<LeanOwned>,
    b: LeanList<LeanOwned>,
    c: LeanList<LeanOwned>,
) -> LeanList<LeanOwned> {
    let mut nats = Vec::new();
    for elem in a.iter() {
        nats.push(Nat::from_obj(&elem));
    }
    for elem in b.iter() {
        nats.push(Nat::from_obj(&elem));
    }
    for elem in c.iter() {
        nats.push(Nat::from_obj(&elem));
    }
    let items: Vec<LeanOwned> = nats.iter().map(build_nat).collect();
    items.into_iter().collect()
    // a, b, c all drop here → lean_dec on each
}

/// Owned ByteArray: reverse the bytes and return.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_reverse_bytearray(
    ba: LeanByteArray<LeanOwned>,
) -> LeanByteArray<LeanOwned> {
    let bytes = ba.as_bytes();
    let reversed: Vec<u8> = bytes.iter().rev().copied().collect();
    LeanByteArray::from_bytes(&reversed)
    // ba drops here → lean_dec
}

/// Owned Point (ctor): negate both fields (swap x and y + add them).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_point_sum(point: LeanCtor<LeanOwned>) -> LeanNat<LeanOwned> {
    let x = Nat::from_obj(&point.get(0));
    let y = Nat::from_obj(&point.get(1));
    Nat(x.0 + y.0).to_lean()
    // point drops here → lean_dec
}

/// Owned Except: if ok, double the nat; if error, return error string length.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_except_transform(
    exc: LeanExcept<LeanOwned>,
) -> LeanNat<LeanOwned> {
    match exc.into_result() {
        Ok(val) => {
            let nat = Nat::from_obj(&val);
            Nat(nat.0.clone() + nat.0).to_lean()
        }
        Err(err) => {
            let s = err.as_string();
            Nat::from(s.byte_len() as u64).to_lean()
        }
    }
    // exc drops here → lean_dec
}

/// Owned Option: if some(n), return n*n; if none, return 0.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_option_square(opt: LeanOption<LeanOwned>) -> LeanNat<LeanOwned> {
    if opt.inner().is_scalar() {
        Nat::ZERO.to_lean()
    } else {
        let val = opt.to_option().unwrap();
        let nat = Nat::from_obj(&val);
        Nat(nat.0.clone() * nat.0).to_lean()
    }
}

/// Owned Prod: return fst * snd.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_prod_multiply(pair: LeanProd<LeanOwned>) -> LeanNat<LeanOwned> {
    let fst = Nat::from_obj(&pair.fst());
    let snd = Nat::from_obj(&pair.snd());
    Nat(fst.0 * snd.0).to_lean()
}

/// Owned ScalarStruct: sum all scalar fields.
/// ScalarStruct { obj : Nat, u8val : UInt8, u32val : UInt32, u64val : UInt64 }
/// Lean reorders scalar fields by descending size:
///   u64val at scalar offset 0, u32val at offset 8, u8val at offset 12
/// Note: roundtrip tests use declaration-order offsets (0, 1, 5) which happen
/// to roundtrip correctly because both read and write use the same offsets.
/// But for computing actual values, we must use the real layout.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_scalar_sum(ptr: LeanScalarStruct<LeanOwned>) -> u64 {
    // Lean descending-size layout: u64(0), u32(8), u8(12)
    let ctor = ptr.as_ctor();
    let u64val = ctor.get_u64(1, 0);
    let u32val = ctor.get_u32(1, 8) as u64;
    let u8val = ctor.get_u8(1, 12) as u64;
    u64val + u32val + u8val
    // ptr drops here → lean_dec
}

// =============================================================================
// Clone tests — verify lean_inc is called correctly
// =============================================================================

/// Clone an owned array and return the sum of lengths of both copies.
/// Tests that Clone (lean_inc) produces a valid second handle.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_array_len_sum(arr_ptr: LeanArray<LeanBorrowed<'_>>) -> usize {
    // Create an owned copy, then clone it
    let owned: LeanArray<LeanOwned> = {
        let nats: Vec<Nat> = arr_ptr.map(|b| Nat::from_obj(&b));
        let arr = LeanArray::alloc(nats.len());
        for (i, nat) in nats.iter().enumerate() {
            arr.set(i, build_nat(nat));
        }
        arr
    };
    let cloned = owned.clone();

    // Both owned and cloned drop here → lean_dec called twice (correct: clone did lean_inc)
    owned.len() + cloned.len()
}

/// Clone an owned string and return the sum of byte lengths of both copies.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_string_len_sum(s: LeanString<LeanBorrowed<'_>>) -> usize {
    let owned = LeanString::new(&s.to_string());
    let cloned = owned.clone();

    owned.byte_len() + cloned.byte_len()
}

/// Clone an owned Except and read from both copies. Tests that lean_inc
/// produces a valid second handle for constructor objects, and that both
/// copies can be independently dropped (lean_dec) without double-free.
/// Returns: for ok(n), 2*n (read n from both copies); for error(s), 2*byte_len.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_except(exc: LeanExcept<LeanOwned>) -> LeanNat<LeanOwned> {
    let cloned = exc.clone();
    let result = match (exc.into_result(), cloned.into_result()) {
        (Ok(v1), Ok(v2)) => Nat(Nat::from_obj(&v1).0 + Nat::from_obj(&v2).0),
        (Err(e1), Err(e2)) => {
            let s1 = e1.as_string();
            let s2 = e2.as_string();
            Nat::from((s1.byte_len() + s2.byte_len()) as u64)
        }
        _ => panic!("clone changed the tag"),
    };
    result.to_lean()
}

/// Clone an owned List, count elements in both copies. Tests lean_inc on list spine.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_list(list: LeanList<LeanOwned>) -> LeanNat<LeanOwned> {
    let cloned = list.clone();
    let count1 = list.iter().count();
    let count2 = cloned.iter().count();
    Nat::from((count1 + count2) as u64).to_lean()
}

/// Clone an owned ByteArray, sum byte lengths of both. Tests lean_inc on scalar arrays.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_bytearray(ba: LeanByteArray<LeanOwned>) -> LeanNat<LeanOwned> {
    let cloned = ba.clone();
    Nat::from((ba.len() + cloned.len()) as u64).to_lean()
}

/// Clone an owned Option Nat: if some(n), return 2*n from both copies; if none, return 0.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_option(opt: LeanOption<LeanOwned>) -> LeanNat<LeanOwned> {
    let cloned = opt.clone();
    let result = match (opt.to_option(), cloned.to_option()) {
        (Some(v1), Some(v2)) => Nat(Nat::from_obj(&v1).0 + Nat::from_obj(&v2).0),
        (None, None) => Nat::ZERO,
        _ => panic!("clone changed some/none"),
    };
    result.to_lean()
}

/// Clone an owned Prod, return fst1+fst2+snd1+snd2 from both copies.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_prod(pair: LeanProd<LeanOwned>) -> LeanNat<LeanOwned> {
    let cloned = pair.clone();
    let sum = Nat::from_obj(&pair.fst()).0
        + Nat::from_obj(&pair.snd()).0
        + Nat::from_obj(&cloned.fst()).0
        + Nat::from_obj(&cloned.snd()).0;
    Nat(sum).to_lean()
}

/// Owned ByteArray roundtrip: read bytes, rebuild. Tests LeanOwned Drop on scalar arrays.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_bytearray_roundtrip(
    ba: LeanByteArray<LeanOwned>,
) -> LeanByteArray<LeanOwned> {
    LeanByteArray::from_bytes(ba.as_bytes())
    // ba drops → lean_dec
}

/// Owned Option roundtrip: decode and re-encode. Tests Drop on Option constructor.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_option_roundtrip(
    opt: LeanOption<LeanOwned>,
) -> LeanOption<LeanOwned> {
    match opt.to_option() {
        None => LeanOption::none(),
        Some(val) => LeanOption::some(Nat::from_obj(&val).to_lean()),
    }
}

/// Owned Prod roundtrip: decode fst/snd, rebuild. Tests Drop on Prod constructor.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_prod_roundtrip(pair: LeanProd<LeanOwned>) -> LeanProd<LeanOwned> {
    let f = Nat::from_obj(&pair.fst());
    let s = Nat::from_obj(&pair.snd());
    LeanProd::new(build_nat(&f), build_nat(&s))
}

/// Owned IOResult: extract value from ok result, return it. Tests Drop on IOResult.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_owned_io_result_value(
    result: LeanIOResult<LeanOwned>,
) -> LeanNat<LeanOwned> {
    // IOResult ok = tag 0, fields: [value, world]; error = tag 1
    let ctor = result.as_ctor();
    if ctor.tag() == 0 {
        Nat::from_obj(&ctor.get(0)).to_lean()
    } else {
        Nat::ZERO.to_lean()
    }
}

// =============================================================================
// data() slice API tests
// =============================================================================

/// Sum all Nats in an array using the data() slice API.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_array_data_sum(
    arr_ptr: LeanArray<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let mut sum = Nat::ZERO;
    for elem in arr_ptr.data() {
        sum = Nat(sum.0 + Nat::from_obj(elem).0);
    }
    sum.to_lean()
}

// =============================================================================
// LeanOption API tests
// =============================================================================

/// Test LeanOption API: return the Nat inside a Some, or 0 for None.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_option_unwrap_or_zero(
    opt: LeanOption<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    match opt.to_option() {
        None => Nat::ZERO.to_lean(),
        Some(val) => Nat::from_obj(&val).to_lean(),
    }
}

// =============================================================================
// LeanProd API tests
// =============================================================================

/// Test LeanProd fst/snd API: swap the elements of a pair.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_prod_swap(pair: LeanProd<LeanBorrowed<'_>>) -> LeanProd<LeanOwned> {
    let fst = Nat::from_obj(&pair.fst());
    let snd = Nat::from_obj(&pair.snd());
    LeanProd::new(build_nat(&snd), build_nat(&fst))
}

// =============================================================================
// Borrowed result (b_lean_obj_res) internal tests
// =============================================================================
// These test the internal pattern where methods return LeanBorrowed<'_> — the
// Rust equivalent of b_lean_obj_res. The borrowed reference is tied to the
// parent object's lifetime, so it cannot outlive the source.

/// Helper: takes a borrowed Prod and returns a borrowed ref to its first element.
/// This is the b_lean_obj_res pattern — returning a reference into an existing object.
fn borrow_fst<'a>(pair: &'a LeanProd<impl LeanRef>) -> LeanBorrowed<'a> {
    pair.fst()
}

/// Helper: takes a borrowed Prod and returns a borrowed ref to its second element.
fn borrow_snd<'a>(pair: &'a LeanProd<impl LeanRef>) -> LeanBorrowed<'a> {
    pair.snd()
}

/// Helper: takes a borrowed array and returns a borrowed ref to element i.
fn borrow_array_elem<'a>(arr: &'a LeanArray<impl LeanRef>, i: usize) -> LeanBorrowed<'a> {
    arr.get(i)
}

/// Helper: takes a borrowed Except and returns a borrowed ref to the inner value.
fn borrow_except_value<'a>(exc: &'a LeanExcept<impl LeanRef>) -> LeanBorrowed<'a> {
    match exc.into_result() {
        Ok(val) => val,
        Err(err) => err,
    }
}

/// Test that chains borrowed results through multiple internal functions.
/// Receives a Prod (Array Nat, Array Nat), borrows fst and snd, then borrows
/// elements from each array, and sums everything — all without any lean_inc.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_borrowed_result_chain(
    pair: LeanProd<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    // Get borrowed references to the two arrays (b_lean_obj_res pattern)
    let fst_ref = borrow_fst(&pair);
    let snd_ref = borrow_snd(&pair);

    // Interpret as arrays (still borrowed, no ref counting)
    let arr1 = fst_ref.as_array();
    let arr2 = snd_ref.as_array();

    // Borrow individual elements from each array (chained b_lean_obj_res)
    let mut sum = Nat::ZERO;
    for i in 0..arr1.len() {
        let elem = borrow_array_elem(&arr1, i);
        sum = Nat(sum.0 + Nat::from_obj(&elem).0);
    }
    for i in 0..arr2.len() {
        let elem = borrow_array_elem(&arr2, i);
        sum = Nat(sum.0 + Nat::from_obj(&elem).0);
    }

    // Also access via data() slice — another b_lean_obj_res pattern
    // (data() returns &[LeanBorrowed] tied to the array's lifetime)
    let mut sum2 = Nat::ZERO;
    for elem in arr1.data() {
        sum2 = Nat(sum2.0 + Nat::from_obj(elem).0);
    }
    for elem in arr2.data() {
        sum2 = Nat(sum2.0 + Nat::from_obj(elem).0);
    }

    assert!(sum == sum2, "get() and data() must agree");
    sum.to_lean()
}

/// Test borrowed result from Except. Borrows the inner value without lean_inc,
/// reads it, and returns a new owned Nat.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_borrowed_except_value(
    exc: LeanExcept<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let val = borrow_except_value(&exc);
    if exc.is_ok() {
        Nat::from_obj(&val).to_lean()
    } else {
        let s = val.as_string();
        Nat::from(s.byte_len() as u64).to_lean()
    }
}

// =============================================================================
// Nested collection tests
// =============================================================================

/// Round-trip an Array (Array Nat) — tests nested ownership.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_nested_array(
    outer: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let len = outer.len();
    let result = LeanArray::alloc(len);
    for i in 0..len {
        let inner_ref = outer.get(i);
        // inner_ref is a LeanBorrowed pointing to an inner Array
        let inner_arr = inner_ref.as_array();
        let inner_len = inner_arr.len();
        let new_inner = LeanArray::alloc(inner_len);
        for j in 0..inner_len {
            let nat = Nat::from_obj(&inner_arr.get(j));
            new_inner.set(j, build_nat(&nat));
        }
        result.set(i, new_inner);
    }
    result
}

/// Round-trip a List (List Nat) — tests nested list iteration.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_roundtrip_nested_list(
    outer: LeanList<LeanBorrowed<'_>>,
) -> LeanList<LeanOwned> {
    let inner_lists: Vec<LeanList<LeanOwned>> = outer.collect(|inner_ref| {
        let inner_list = inner_ref.as_list();
        let nats: Vec<Nat> = inner_list.collect(|b| Nat::from_obj(&b));
        let items: Vec<LeanOwned> = nats.iter().map(build_nat).collect();
        items.into_iter().collect()
    });
    inner_lists.into_iter().collect()
}

// =============================================================================
// LeanExcept into_result API test
// =============================================================================

/// Test LeanExcept-like pattern: if ok (tag 1), return nat + 1; if error (tag 0), return 0.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_except_map_ok(exc: LeanExcept<LeanBorrowed<'_>>) -> LeanNat<LeanOwned> {
    let ctor = exc.as_ctor();
    if ctor.tag() == 1 {
        // ok: field 0 is the Nat value
        let nat = Nat::from_obj(&ctor.get(0));
        Nat(nat.0 + 1u64).to_lean()
    } else {
        // error
        Nat::ZERO.to_lean()
    }
}

// =============================================================================
// Multiple borrow test — read many elements from same borrowed source
// =============================================================================

/// Read all elements from a borrowed array, compute sum.
/// Tests that multiple borrows from the same source don't interfere.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_multi_borrow_sum(
    arr: LeanArray<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let mut sum = Nat::ZERO;
    // First pass: read all via get()
    for i in 0..arr.len() {
        let elem = arr.get(i);
        sum = Nat(sum.0 + Nat::from_obj(&elem).0);
    }
    // Second pass: read all via data() slice
    let mut sum2 = Nat::ZERO;
    for elem in arr.data() {
        sum2 = Nat(sum2.0 + Nat::from_obj(elem).0);
    }
    // Third pass: read via iter()
    let mut sum3 = Nat::ZERO;
    for elem in arr.iter() {
        sum3 = Nat(sum3.0 + Nat::from_obj(&elem).0);
    }
    assert!(
        sum == sum2 && sum2 == sum3,
        "All three iteration methods must agree"
    );
    sum.to_lean()
}

// =============================================================================
// Build array from list using push — exercises ownership transfer chain
// =============================================================================

/// Convert List Nat → Array Nat using only push (not alloc+set).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_list_to_array_via_push(
    list: LeanList<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let mut arr = LeanArray::alloc(0);
    for elem in list.iter() {
        let nat = Nat::from_obj(&elem);
        arr = arr.push(build_nat(&nat));
    }
    arr
}

// =============================================================================
// to_owned_ref test — convert borrowed to owned explicitly
// =============================================================================

/// Take a borrowed Nat, convert to owned via to_owned_ref, return it.
/// Tests that to_owned_ref (lean_inc) produces a valid owned handle.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_borrow_to_owned(nat: LeanNat<LeanBorrowed<'_>>) -> LeanNat<LeanOwned> {
    LeanNat::new(nat.inner().to_owned_ref())
}

// =============================================================================
// Empty collection edge cases
// =============================================================================

/// Create and return an empty array. Unit is passed as lean_box(0).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_make_empty_array(_unit: LeanBorrowed<'_>) -> LeanArray<LeanOwned> {
    LeanArray::alloc(0)
}

/// Create and return an empty list.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_make_empty_list(_unit: LeanBorrowed<'_>) -> LeanList<LeanOwned> {
    LeanList::nil()
}

/// Create and return an empty byte array.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_make_empty_bytearray(
    _unit: LeanBorrowed<'_>,
) -> LeanByteArray<LeanOwned> {
    LeanByteArray::alloc(0)
}

/// Create and return an empty string.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_make_empty_string(_unit: LeanBorrowed<'_>) -> LeanString<LeanOwned> {
    LeanString::new("")
}

// =============================================================================
// Scalar boundary values
// =============================================================================

/// Return the Nat boundary between scalar and heap representation.
/// On 64-bit: usize::MAX >> 1 = 2^63 - 1
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_nat_max_scalar(_unit: LeanBorrowed<'_>) -> LeanNat<LeanOwned> {
    let max_scalar = usize::MAX >> 1;
    LeanNat::new(LeanOwned::box_usize(max_scalar))
}

/// Return max_scalar + 1 which must be heap-allocated.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_nat_min_heap(_unit: LeanBorrowed<'_>) -> LeanNat<LeanOwned> {
    let max_scalar = (usize::MAX >> 1) as u64;
    Nat::from(max_scalar + 1).to_lean()
}

// =============================================================================
// String length, Array/List conversion, ByteArray copy
// =============================================================================

/// Return the string length (character count). Wraps `LeanString::length`.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_string_length(s: LeanString<LeanBorrowed<'_>>) -> usize {
    s.length()
}

/// Round-trip: Array → List → Array. Tests from_list and to_list.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_array_list_roundtrip(
    arr: LeanArray<LeanBorrowed<'_>>,
) -> LeanArray<LeanOwned> {
    let list = arr.inner().to_owned_ref();
    let arr = unsafe { LeanArray::from_raw(list.into_raw()) };
    let list = arr.to_list();
    LeanArray::from_list(list)
}

/// Copy a byte array, mutate the copy, return the copy.
/// Tests that copy produces an independent array.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_bytearray_copy_mutate(
    ba: LeanByteArray<LeanOwned>,
) -> LeanByteArray<LeanOwned> {
    let copy = ba.copy();
    copy.uset(0, 255)
}

// =============================================================================
// In-place mutation tests (uset, pop, uswap, push, get_mut)
// =============================================================================

/// Exercise array mutation operations: uset, uswap, pop, push.
/// Input: [1, 2, 3, 4] → uset [0]=10 → [10, 2, 3, 4]
///                       → uswap 1 3  → [10, 4, 3, 2]
///                       → pop        → [10, 4, 3]
///                       → push 99    → [10, 4, 3, 99]
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_array_mut_ops(arr: LeanArray<LeanOwned>) -> LeanArray<LeanOwned> {
    let arr = arr.uset(0, LeanOwned::from_nat_u64(10));
    let arr = arr.uswap(1, 3);
    let arr = arr.pop();
    arr.push(LeanOwned::from_nat_u64(99))
}

/// Exercise byte array mutation operations: uset and push.
/// Input: [1, 2, 3] → uset [0]=255 → [255, 2, 3] → push 42 → [255, 2, 3, 42]
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_bytearray_mut_ops(
    ba: LeanByteArray<LeanOwned>,
) -> LeanByteArray<LeanOwned> {
    let ba = ba.uset(0, 255);
    ba.push(42)
}

/// Test full external object lifecycle: create → read → mutate → read.
/// Allocates in Rust (rc=1, guaranteed exclusive), reads initial state,
/// mutates x via get_mut, then verifies x changed and y/label are preserved.
/// Returns "before/after" as "x:y:label/x:y:label".
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_lifecycle(
    x: u64,
    y: u64,
    label: LeanString<LeanBorrowed<'_>>,
    new_x: u64,
) -> LeanString<LeanOwned> {
    let data = RustData {
        x,
        y,
        label: label.to_string(),
    };
    // Create
    let mut ext = LeanExternal::alloc(&RUST_DATA_CLASS, data);
    // Read
    let before = format!("{}:{}:{}", ext.get().x, ext.get().y, ext.get().label);
    // Update
    if let Some(data) = ext.get_mut() {
        data.x = new_x;
    }
    // Read again — x changed, y and label preserved
    let after = format!("{}:{}:{}", ext.get().x, ext.get().y, ext.get().label);
    // Delete — ext drops here via lean_dec → finalizer → Drop
    LeanString::new(&format!("{before}/{after}"))
}

/// Mutate a string: append a suffix, then push '!'.
/// Chaining from Lean tests refcounting across calls.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_string_mut_ops(
    s: LeanString<LeanOwned>,
    suffix: LeanString<LeanBorrowed<'_>>,
) -> LeanString<LeanOwned> {
    let s = s.append(&suffix);
    s.push(u32::from('!'))
}

/// Update the x field of a RustData external, returning the modified object.
/// Uses get_mut for in-place mutation when exclusive, clones otherwise.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_set_x(
    obj: LeanRustData<LeanOwned>,
    new_x: u64,
) -> LeanRustData<LeanOwned> {
    let mut ext = unsafe { LeanExternal::<RustData, LeanOwned>::from_raw(obj.into_raw()) };
    if let Some(data) = ext.get_mut() {
        data.x = new_x;
        LeanRustData::new(ext.into())
    } else {
        let mut data = ext.get().clone();
        data.x = new_x;
        LeanRustData::new(LeanExternal::alloc(&RUST_DATA_CLASS, data).into())
    }
}

// =============================================================================
// External object: multiple field reads from same borrowed handle
// =============================================================================

/// Read all fields from a single borrowed external handle and return as a string.
/// Tests that multiple reads from a borrowed external don't corrupt state.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_external_all_fields(
    obj: LeanRustData<LeanBorrowed<'_>>,
) -> LeanString<LeanOwned> {
    let ext =
        unsafe { LeanExternal::<RustData, LeanBorrowed<'_>>::from_raw_borrowed(obj.as_raw()) };
    let result = format!("{}:{}:{}", ext.get().x, ext.get().y, ext.get().label);
    LeanString::new(&result)
}

// =============================================================================
// Memory management stress tests (Valgrind targets)
// =============================================================================
// These tests allocate and drop objects in Rust without returning them to Lean.
// Valgrind detects leaks (missing lean_dec), double-frees (extra lean_dec),
// or use-after-free from incorrect ownership transfer.

/// Allocate every object type in Rust and drop them all without returning to Lean.
/// Tests that Drop impls correctly call lean_dec and that external finalizers
/// free the boxed Rust data.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_alloc_drop_stress(_unit: LeanBorrowed<'_>) -> u8 {
    // Array with elements
    let arr = LeanArray::alloc(3);
    arr.set(0, LeanOwned::from_nat_u64(1));
    arr.set(1, LeanOwned::from_nat_u64(2));
    arr.set(2, LeanOwned::from_nat_u64(3));

    // ByteArray
    let _ba = LeanByteArray::from_bytes(&[1, 2, 3, 4, 5]);

    // String
    let _s = LeanString::new("hello world");

    // External — finalizer must run Drop on RustData (freeing the String inside)
    let _ext = LeanExternal::alloc(
        &RUST_DATA_CLASS,
        RustData {
            x: 42,
            y: 99,
            label: String::from("this string must be freed by the finalizer"),
        },
    );

    // List (nil)
    let _nil = LeanList::nil();

    // Nat (heap-allocated, not scalar)
    let _nat = LeanOwned::from_nat_u64(u64::MAX);

    // All variables drop here at end of scope
    1
}

/// Chain mutations in Rust where each step frees the previous object.
/// Tests that uset/push/pop/uswap correctly dec the consumed array.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_mutation_drop_stress(_unit: LeanBorrowed<'_>) -> u8 {
    // Array: each mutation consumes the old array
    let arr = LeanArray::alloc(3);
    arr.set(0, LeanOwned::from_nat_u64(10));
    arr.set(1, LeanOwned::from_nat_u64(20));
    arr.set(2, LeanOwned::from_nat_u64(30));
    let arr = arr.push(LeanOwned::from_nat_u64(40));
    let arr = arr.uset(0, LeanOwned::from_nat_u64(99));
    let arr = arr.uswap(0, 2);
    let _arr = arr.pop();

    // ByteArray: push and uset chain
    let ba = LeanByteArray::from_bytes(&[1, 2, 3]);
    let ba = ba.push(4);
    let _ba = ba.uset(0, 255);

    // String: append and push chain
    let s = LeanString::new("hello");
    let suffix = LeanString::new(" world");
    let s = s.append(&suffix);
    let _s = s.push(u32::from('!'));
    // suffix also drops here

    1
}

/// Clone a borrowed array N times, read from each clone, drop all.
/// Tests that lean_inc (via to_owned_ref) and lean_dec (via Drop) are balanced.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_clone_drop_stress(arr: LeanArray<LeanBorrowed<'_>>, n: usize) -> usize {
    let mut total_len = 0;
    for _ in 0..n {
        let owned = arr.inner().to_owned_ref(); // lean_inc
        total_len += unsafe { include::lean_array_size(owned.as_raw()) };
        // owned drops → lean_dec
    }
    total_len
}

// =============================================================================
// Persistent / compact region tests
// =============================================================================
// Persistent objects have m_rc == 0 and are never deallocated. They arise from
// module-level definitions and compact regions. Borrowed references to them
// work normally; the key invariant is that lean_inc/lean_dec are no-ops.

/// Check if a borrowed Lean object is persistent (m_rc == 0).
/// Module-level Lean definitions become persistent after initialization.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_is_persistent(obj: LeanNat<LeanBorrowed<'_>>) -> u8 {
    if obj.inner().is_persistent() { 1 } else { 0 }
}

/// Read a Nat from a persistent object (passed as @& borrowed).
/// Tests that field access works normally on persistent objects.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_read_persistent_nat(
    obj: LeanNat<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let nat = Nat::from_obj(obj.inner());
    nat.to_lean()
}

/// Read fields from a persistent LeanPoint (structure with x, y : Nat).
/// Tests that ctor field access works on persistent objects.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_read_persistent_point(
    point: LeanPoint<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let ctor = point.as_ctor();
    let x = Nat::from_obj(&ctor.get(0));
    let y = Nat::from_obj(&ctor.get(1));
    // Return x + y as a new (non-persistent) Nat
    Nat(x.0 + y.0).to_lean()
}

/// Read from a persistent array. Tests that array element access works
/// on persistent objects (elements in persistent arrays are also persistent).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_read_persistent_array(
    arr: LeanArray<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let mut sum = Nat::ZERO;
    for elem in arr.iter() {
        sum = Nat(sum.0 + Nat::from_obj(&elem).0);
    }
    sum.to_lean()
}

/// Read from a persistent string. Tests that string access works on persistent objects.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_read_persistent_string(
    s: LeanString<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    Nat::from(s.byte_len() as u64).to_lean()
}

/// Receive a persistent object as owned (lean_obj_arg). Lean transfers a
/// "virtual RC token" but lean_dec is a no-op for persistent objects (m_rc == 0).
/// This tests that LeanOwned::drop doesn't crash on persistent data.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_drop_persistent_nat(obj: LeanNat<LeanOwned>) -> LeanNat<LeanOwned> {
    let nat = Nat::from_obj(obj.inner());
    nat.to_lean()
    // obj drops here → lean_dec_ref → no-op because m_rc == 0
}

// =============================================================================
// LeanShared — thread-safe multi-threaded refcounting tests
// =============================================================================

use crate::LeanShared;

/// Mark an array as MT via LeanShared, clone it across N threads,
/// each thread reads all elements, then all clones are dropped.
/// Tests that lean_mark_mt + atomic refcounting works correctly.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_parallel_read(
    arr: LeanArray<LeanBorrowed<'_>>,
    n_threads: usize,
) -> LeanNat<LeanOwned> {
    use std::thread;

    // Create an owned copy and mark as MT
    let shared = LeanShared::new(arr.inner().to_owned_ref());

    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let shared_clone = shared.clone(); // atomic lean_inc
        handles.push(thread::spawn(move || {
            // Each thread borrows and reads all elements
            let borrowed = shared_clone.borrow().as_array();
            let mut sum: u64 = 0;
            for elem in borrowed.iter() {
                sum += Nat::from_obj(&elem).to_u64().unwrap_or(0);
            }
            sum
            // shared_clone dropped → atomic lean_dec
        }));
    }

    let mut total: u64 = 0;
    for h in handles {
        total += h.join().unwrap();
    }
    // shared dropped → atomic lean_dec (last ref frees)

    Nat::from(total).to_lean()
}

/// Mark a Nat as MT, clone it to N threads, each reads it.
/// Simpler than array — tests basic scalar/heap Nat across threads.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_parallel_nat(
    nat: LeanNat<LeanBorrowed<'_>>,
    n_threads: usize,
) -> LeanNat<LeanOwned> {
    use std::thread;

    let shared = LeanShared::new(nat.inner().to_owned_ref());

    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let shared_clone = shared.clone();
        handles.push(thread::spawn(move || Nat::from_obj(&shared_clone.borrow())));
    }

    // All threads should read the same value
    let results: Vec<Nat> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &results[0];
    assert!(results.iter().all(|r| r == first), "MT read inconsistency");

    first.to_lean()
}

/// Mark a string as MT, clone to N threads, each reads byte_len.
/// Returns sum of all byte_len readings.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_parallel_string(
    s: LeanString<LeanBorrowed<'_>>,
    n_threads: usize,
) -> LeanNat<LeanOwned> {
    use std::thread;

    let shared = LeanShared::new(s.inner().to_owned_ref());

    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let shared_clone = shared.clone();
        handles.push(thread::spawn(move || {
            shared_clone.borrow().as_string().byte_len() as u64
        }));
    }

    let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    Nat::from(total).to_lean()
}

/// Stress test: mark array as MT, spawn many threads that each clone
/// and drop rapidly. Tests atomic refcount under contention.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_contention_stress(
    arr: LeanArray<LeanBorrowed<'_>>,
    n_threads: usize,
    clones_per_thread: usize,
) -> LeanNat<LeanOwned> {
    use std::thread;

    let shared = LeanShared::new(arr.inner().to_owned_ref());

    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let shared_clone = shared.clone();
        handles.push(thread::spawn(move || {
            // Rapidly clone and drop to stress atomic refcount
            for _ in 0..clones_per_thread {
                let tmp = shared_clone.clone();
                let _ = tmp.borrow().as_array().len();
                // tmp dropped → atomic lean_dec
            }
            shared_clone.borrow().as_array().len() as u64
        }));
    }

    let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    Nat::from(total).to_lean()
}

/// Test into_owned: mark as MT, convert back to LeanOwned, read value.
/// Verifies the MT-marked object is still usable after unwrapping.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_into_owned(
    nat: LeanNat<LeanBorrowed<'_>>,
) -> LeanNat<LeanOwned> {
    let shared = LeanShared::new(nat.inner().to_owned_ref());
    let cloned = shared.clone();
    // Convert one back to LeanOwned
    let owned = cloned.into_owned();
    let val = Nat::from_obj(&unsafe { LeanBorrowed::from_raw(owned.as_raw()) });

    // owned drops (still MT-marked, lean_dec_ref handles it)
    // shared drops (atomic lean_dec)
    val.to_lean()
}

/// Mark a Point (constructor with 2 obj fields) as MT, read fields from threads.
/// Tests that lean_mark_mt correctly walks the constructor's object graph.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_parallel_point(
    point: LeanPoint<LeanBorrowed<'_>>,
    n_threads: usize,
) -> LeanNat<LeanOwned> {
    use std::thread;

    let shared = LeanShared::new(point.inner().to_owned_ref());

    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let shared_clone = shared.clone();
        handles.push(thread::spawn(move || {
            let ctor = shared_clone.borrow().as_ctor();
            let x = Nat::from_obj(&ctor.get(0));
            let y = Nat::from_obj(&ctor.get(1));
            x.to_u64().unwrap_or(0) + y.to_u64().unwrap_or(0)
        }));
    }

    let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    Nat::from(total).to_lean()
}

/// Wrap a persistent Nat in LeanShared (lean_mark_mt is skipped for persistent).
/// Clone to threads and read — verifies the persistent skip path works.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rs_shared_persistent_nat(
    nat: LeanNat<LeanBorrowed<'_>>,
    n_threads: usize,
) -> LeanNat<LeanOwned> {
    use std::thread;

    // For persistent objects, LeanShared::new skips lean_mark_mt.
    // lean_inc_ref / lean_dec_ref are already no-ops for m_rc == 0,
    // so Clone and Drop are safe without MT marking.
    let shared = LeanShared::new(nat.inner().to_owned_ref());

    let mut handles = Vec::new();
    for _ in 0..n_threads {
        let shared_clone = shared.clone();
        handles.push(thread::spawn(move || Nat::from_obj(&shared_clone.borrow())));
    }

    let results: Vec<Nat> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let first = &results[0];
    assert!(
        results.iter().all(|r| r == first),
        "persistent MT read inconsistency"
    );
    first.to_lean()
}
