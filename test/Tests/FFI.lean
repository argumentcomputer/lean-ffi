/-
  FFI roundtrip tests for lean-ffi.
  Pattern: Lean value → Rust (decode via lean-ffi) → Rust (re-encode) → Lean value → compare
-/
module

public import LSpec
public import Tests.Gen

open LSpec SlimCheck Gen

namespace Tests.FFI

/-! ## FFI declarations -/

@[extern "rs_roundtrip_nat"]
opaque roundtripNat : @& Nat → Nat

@[extern "rs_roundtrip_string"]
opaque roundtripString : @& String → String

@[extern "rs_roundtrip_bool"]
opaque roundtripBool : @& Bool → Bool

@[extern "rs_roundtrip_list_nat"]
opaque roundtripListNat : @& List Nat → List Nat

@[extern "rs_roundtrip_array_nat"]
opaque roundtripArrayNat : @& Array Nat → Array Nat

@[extern "rs_roundtrip_bytearray"]
opaque roundtripByteArray : @& ByteArray → ByteArray

@[extern "rs_roundtrip_option_nat"]
opaque roundtripOptionNat : @& Option Nat → Option Nat

@[extern "rs_roundtrip_point"]
opaque roundtripPoint : @& Point → Point

@[extern "rs_roundtrip_nat_tree"]
opaque roundtripNatTree : @& NatTree → NatTree

@[extern "rs_roundtrip_prod_nat_nat"]
opaque roundtripProdNatNat : @& Nat × Nat → Nat × Nat

@[extern "rs_roundtrip_except_string_nat"]
opaque roundtripExceptStringNat : @& Except String Nat → Except String Nat

@[extern "rs_except_error_string"]
opaque exceptErrorString : @& String → Except String Nat

@[extern "rs_io_result_ok_nat"]
opaque ioResultOkNat : @& Nat → EStateM.Result IO.Error PUnit Nat

@[extern "rs_io_result_error_string"]
opaque ioResultErrorString : @& String → EStateM.Result IO.Error PUnit Nat

@[extern "rs_roundtrip_scalar_struct"]
opaque roundtripScalarStruct : @& ScalarStruct → ScalarStruct

@[extern "rs_roundtrip_ext_scalar_struct"]
opaque roundtripExtScalarStruct : @& ExtScalarStruct → ExtScalarStruct

@[extern "rs_roundtrip_usize_struct"]
opaque roundtripUSizeStruct : @& USizeStruct → USizeStruct

@[extern "rs_roundtrip_float"]
opaque roundtripFloat : Float → Float

@[extern "rs_roundtrip_float32"]
opaque roundtripFloat32 : Float32 → Float32

@[extern "rs_roundtrip_array_float"]
opaque roundtripArrayFloat : @& Array Float → Array Float

@[extern "rs_roundtrip_array_float32"]
opaque roundtripArrayFloat32 : @& Array Float32 → Array Float32

@[extern "rs_roundtrip_usize"]
opaque roundtripUSize : USize → USize

@[extern "rs_roundtrip_string_from_bytes"]
opaque roundtripStringFromBytes : @& String → String

@[extern "rs_roundtrip_array_push"]
opaque roundtripArrayPush : @& Array Nat → Array Nat

@[extern "rs_roundtrip_uint32"]
opaque roundtripUInt32 : UInt32 → UInt32

@[extern "rs_roundtrip_uint64"]
opaque roundtripUInt64 : UInt64 → UInt64

@[extern "rs_roundtrip_array_uint32"]
opaque roundtripArrayUInt32 : @& Array UInt32 → Array UInt32

@[extern "rs_roundtrip_array_uint64"]
opaque roundtripArrayUInt64 : @& Array UInt64 → Array UInt64

/-- Opaque type representing Rust-owned data behind a Lean external object -/
opaque RustDataPointed : NonemptyType
def RustData : Type := RustDataPointed.type
instance : Nonempty RustData := RustDataPointed.property

@[extern "rs_external_create"]
opaque mkRustData : UInt64 → UInt64 → @& String → RustData

@[extern "rs_external_get_x"]
opaque rustDataGetX : @& RustData → UInt64

@[extern "rs_external_get_y"]
opaque rustDataGetY : @& RustData → UInt64

@[extern "rs_external_get_label"]
opaque rustDataGetLabel : @& RustData → String

/-! ## Unit tests -/

def simpleTests : TestSeq :=
  test "Nat 0" (roundtripNat 0 == 0) ++
  test "Nat 42" (roundtripNat 42 == 42) ++
  test "Nat 1000" (roundtripNat 1000 == 1000) ++
  test "String empty" (roundtripString "" == "") ++
  test "String hello" (roundtripString "hello" == "hello") ++
  test "Bool true" (roundtripBool true == true) ++
  test "Bool false" (roundtripBool false == false) ++
  test "List []" (roundtripListNat [] == []) ++
  test "List [1,2,3]" (roundtripListNat [1, 2, 3] == [1, 2, 3]) ++
  test "Array #[]" (roundtripArrayNat #[] == #[]) ++
  test "Array #[1,2,3]" (roundtripArrayNat #[1, 2, 3] == #[1, 2, 3]) ++
  test "ByteArray empty" (roundtripByteArray ⟨#[]⟩ == ⟨#[]⟩) ++
  test "ByteArray [1,2,3]" (roundtripByteArray ⟨#[1, 2, 3]⟩ == ⟨#[1, 2, 3]⟩) ++
  test "Option none" (roundtripOptionNat none == none) ++
  test "Option some 42" (roundtripOptionNat (some 42) == some 42) ++
  test "Point (0, 0)" (roundtripPoint ⟨0, 0⟩ == ⟨0, 0⟩) ++
  test "Point (42, 99)" (roundtripPoint ⟨42, 99⟩ == ⟨42, 99⟩) ++
  test "NatTree leaf" (roundtripNatTree (.leaf 42) == .leaf 42) ++
  test "NatTree node" (roundtripNatTree (.node (.leaf 1) (.leaf 2)) == .node (.leaf 1) (.leaf 2)) ++
  test "Prod (1, 2)" (roundtripProdNatNat (1, 2) == (1, 2)) ++
  test "Prod (0, 0)" (roundtripProdNatNat (0, 0) == (0, 0)) ++
  test "UInt32 0" (roundtripUInt32 0 == 0) ++
  test "UInt32 42" (roundtripUInt32 42 == 42) ++
  test "UInt32 max" (roundtripUInt32 0xFFFFFFFF == 0xFFFFFFFF) ++
  test "UInt64 0" (roundtripUInt64 0 == 0) ++
  test "UInt64 42" (roundtripUInt64 42 == 42) ++
  test "UInt64 max" (roundtripUInt64 0xFFFFFFFFFFFFFFFF == 0xFFFFFFFFFFFFFFFF) ++
  test "Array UInt32 empty" (roundtripArrayUInt32 #[] == #[]) ++
  test "Array UInt32 [1,2,3]" (roundtripArrayUInt32 #[1, 2, 3] == #[1, 2, 3]) ++
  test "Array UInt32 [0, max]" (roundtripArrayUInt32 #[0, 0xFFFFFFFF] == #[0, 0xFFFFFFFF]) ++
  test "Array UInt64 empty" (roundtripArrayUInt64 #[] == #[]) ++
  test "Array UInt64 [1,2,3]" (roundtripArrayUInt64 #[1, 2, 3] == #[1, 2, 3]) ++
  test "Array UInt64 [0, max]" (roundtripArrayUInt64 #[0, 0xFFFFFFFFFFFFFFFF] == #[0, 0xFFFFFFFFFFFFFFFF]) ++
  test "Float 0.0" (roundtripFloat 0.0 == 0.0) ++
  test "Float 3.14" (roundtripFloat 3.14 == 3.14) ++
  test "Float -1.5" (roundtripFloat (-1.5) == -1.5) ++
  test "Float32 0.0" (roundtripFloat32 0.0 == 0.0) ++
  test "Float32 3.14" (roundtripFloat32 3.14 == 3.14) ++
  test "USize 0" (roundtripUSize 0 == 0) ++
  test "USize 42" (roundtripUSize 42 == 42) ++
  test "Array Float [1.5, 2.5]" (roundtripArrayFloat #[1.5, 2.5] == #[1.5, 2.5]) ++
  test "Array Float32 [1.5, 2.5]" (roundtripArrayFloat32 #[1.5, 2.5] == #[1.5, 2.5]) ++
  test "String from_bytes empty" (roundtripStringFromBytes "" == "") ++
  test "String from_bytes hello" (roundtripStringFromBytes "hello" == "hello") ++
  test "Array push empty" (roundtripArrayPush #[] == #[]) ++
  test "Array push [1,2,3]" (roundtripArrayPush #[1, 2, 3] == #[1, 2, 3])

/-! ## Except tests -/

def exceptTests : TestSeq :=
  test "Except.ok 42" (show Bool from
    match roundtripExceptStringNat (.ok 42) with
    | .ok n => n == 42
    | .error _ => false) ++
  test "Except.error hello" (show Bool from
    match roundtripExceptStringNat (.error "hello") with
    | .error s => s == "hello"
    | .ok _ => false) ++
  test "Except.error_string" (show Bool from
    match exceptErrorString "boom" with
    | .error s => s == "boom"
    | .ok _ => false)

/-! ## IO result tests -/

def ioResultTests : TestSeq :=
  test "IOResult ok 42" (show Bool from
    match ioResultOkNat 42 with
    | .ok val _ => val == 42
    | .error _ _ => false) ++
  test "IOResult error" (show Bool from
    match ioResultErrorString "oops" with
    | .error _ _ => true
    | .ok _ _ => false)

/-! ## Scalar struct tests -/

def scalarStructTests : TestSeq :=
  test "ScalarStruct (0, 0, 0, 0)" (roundtripScalarStruct ⟨0, 0, 0, 0⟩ == ⟨0, 0, 0, 0⟩) ++
  test "ScalarStruct (42, 255, 1000, 9999)" (roundtripScalarStruct ⟨42, 255, 1000, 9999⟩ == ⟨42, 255, 1000, 9999⟩) ++
  test "ScalarStruct max vals" (roundtripScalarStruct ⟨100, 0xFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF⟩ == ⟨100, 0xFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF⟩)

/-! ## Extended scalar struct tests -/

def extScalarStructTests : TestSeq :=
  test "ExtScalarStruct zeros" (show Bool from roundtripExtScalarStruct ⟨0, 0, 0, 0, 0, 0.0, 0.0⟩ == ⟨0, 0, 0, 0, 0, 0.0, 0.0⟩) ++
  test "ExtScalarStruct mixed" (show Bool from roundtripExtScalarStruct ⟨42, 255, 1000, 50000, 9999, 3.14, 2.5⟩ == ⟨42, 255, 1000, 50000, 9999, 3.14, 2.5⟩) ++
  test "ExtScalarStruct max ints" (show Bool from roundtripExtScalarStruct ⟨100, 0xFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 1.0, 1.0⟩ == ⟨100, 0xFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 1.0, 1.0⟩)

/-! ## USize struct tests -/

def usizeStructTests : TestSeq :=
  test "USizeStruct zeros" (roundtripUSizeStruct ⟨0, 0, 0⟩ == ⟨0, 0, 0⟩) ++
  test "USizeStruct mixed" (roundtripUSizeStruct ⟨42, 99, 255⟩ == ⟨42, 99, 255⟩)

/-! ## External object tests -/

def externalTests : TestSeq :=
  test "External create and get x" (rustDataGetX (mkRustData 42 99 "hello") == 42) ++
  test "External create and get y" (rustDataGetY (mkRustData 42 99 "hello") == 99) ++
  test "External create and get label" (rustDataGetLabel (mkRustData 42 99 "hello") == "hello") ++
  test "External zero values" (rustDataGetX (mkRustData 0 0 "") == 0) ++
  test "External empty label" (rustDataGetLabel (mkRustData 0 0 "") == "") ++
  test "External large values" (rustDataGetX (mkRustData 0xFFFFFFFFFFFFFFFF 0 "test") == 0xFFFFFFFFFFFFFFFF)

/-! ## Edge cases for large Nats -/

def largeNatTests : TestSeq :=
  let testCases : List Nat := [0, 1, 255, 256, 65535, 65536, (2^32 - 1), 2^32,
    (2^63 - 1), 2^63, (2^64 - 1), 2^64, 2^64 + 1, 2^128, 2^256]
  testCases.foldl (init := .done) fun acc n =>
    acc ++ .individualIO s!"Nat {n}" none (do
      let rt := roundtripNat n
      pure (rt == n, 0, 0, if rt == n then none else some s!"got {rt}")) .done

/-! ## Property-based tests -/

public def suite : List TestSeq := [
  simpleTests,
  largeNatTests,
  exceptTests,
  ioResultTests,
  scalarStructTests,
  extScalarStructTests,
  usizeStructTests,
  externalTests,
  checkIO "Nat roundtrip" (∀ n : Nat, roundtripNat n == n),
  checkIO "String roundtrip" (∀ s : String, roundtripString s == s),
  checkIO "List Nat roundtrip" (∀ xs : List Nat, roundtripListNat xs == xs),
  checkIO "Array Nat roundtrip" (∀ arr : Array Nat, roundtripArrayNat arr == arr),
  checkIO "ByteArray roundtrip" (∀ ba : ByteArray, roundtripByteArray ba == ba),
  checkIO "Option Nat roundtrip" (∀ o : Option Nat, roundtripOptionNat o == o),
  checkIO "Point roundtrip" (∀ p : Point, roundtripPoint p == p),
  checkIO "NatTree roundtrip" (∀ t : NatTree, roundtripNatTree t == t),
]

end Tests.FFI
