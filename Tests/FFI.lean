/-
  FFI roundtrip tests for lean-ffi.
  Pattern: Lean value → Rust (decode via lean-ffi) → Rust (re-encode) → Lean value → compare
-/
module

public import LSpec
public import Tests.Gen

open LSpec SlimCheck Gen

namespace Tests.FFI

/-! ## FFI declarations — borrowed (@&) parameters -/

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

/-! ## FFI declarations — owned parameters (NO @&, Rust must lean_dec) -/

@[extern "rs_owned_nat_roundtrip"]
opaque ownedNatRoundtrip : Nat → Nat

@[extern "rs_owned_string_roundtrip"]
opaque ownedStringRoundtrip : String → String

@[extern "rs_owned_array_nat_roundtrip"]
opaque ownedArrayNatRoundtrip : Array Nat → Array Nat

@[extern "rs_owned_list_nat_roundtrip"]
opaque ownedListNatRoundtrip : List Nat → List Nat

@[extern "rs_owned_append_nat"]
opaque ownedAppendNat : Array Nat → Nat → Array Nat

@[extern "rs_owned_drop_and_replace"]
opaque ownedDropAndReplace : String → String

@[extern "rs_owned_merge_lists"]
opaque ownedMergeLists : List Nat → List Nat → List Nat → List Nat

@[extern "rs_owned_reverse_bytearray"]
opaque ownedReverseByteArray : ByteArray → ByteArray

@[extern "rs_owned_point_sum"]
opaque ownedPointSum : Point → Nat

@[extern "rs_owned_except_transform"]
opaque ownedExceptTransform : Except String Nat → Nat

@[extern "rs_owned_option_square"]
opaque ownedOptionSquare : Option Nat → Nat

@[extern "rs_owned_prod_multiply"]
opaque ownedProdMultiply : Nat × Nat → Nat

@[extern "rs_owned_scalar_sum"]
opaque ownedScalarSum : ScalarStruct → UInt64

/-! ## FFI declarations — Clone, data(), and API tests -/

@[extern "rs_clone_array_len_sum"]
opaque cloneArrayLenSum : @& Array Nat → USize

@[extern "rs_clone_string_len_sum"]
opaque cloneStringLenSum : @& String → USize

@[extern "rs_clone_except"]
opaque cloneExcept : Except String Nat → Nat

@[extern "rs_clone_list"]
opaque cloneList : List Nat → Nat

@[extern "rs_clone_bytearray"]
opaque cloneByteArray : ByteArray → Nat

@[extern "rs_clone_option"]
opaque cloneOption : Option Nat → Nat

@[extern "rs_clone_prod"]
opaque cloneProd : Nat × Nat → Nat

@[extern "rs_owned_bytearray_roundtrip"]
opaque ownedByteArrayRoundtrip : ByteArray → ByteArray

@[extern "rs_owned_option_roundtrip"]
opaque ownedOptionRoundtrip : Option Nat → Option Nat

@[extern "rs_owned_prod_roundtrip"]
opaque ownedProdRoundtrip : Nat × Nat → Nat × Nat

@[extern "rs_owned_io_result_value"]
opaque ownedIOResultValue : EStateM.Result IO.Error PUnit Nat → Nat

@[extern "rs_array_data_sum"]
opaque arrayDataSum : @& Array Nat → Nat

@[extern "rs_option_unwrap_or_zero"]
opaque optionUnwrapOrZero : @& Option Nat → Nat

@[extern "rs_prod_swap"]
opaque prodSwap : @& Nat × Nat → Nat × Nat

@[extern "rs_except_map_ok"]
opaque exceptMapOk : @& Except String Nat → Nat

@[extern "rs_borrowed_result_chain"]
opaque borrowedResultChain : @& Array Nat × Array Nat → Nat

@[extern "rs_borrowed_except_value"]
opaque borrowedExceptValue : @& Except String Nat → Nat

/-! ## FFI declarations — nested collections -/

@[extern "rs_roundtrip_nested_array"]
opaque roundtripNestedArray : @& Array (Array Nat) → Array (Array Nat)

@[extern "rs_roundtrip_nested_list"]
opaque roundtripNestedList : @& List (List Nat) → List (List Nat)

/-! ## FFI declarations — misc edge cases -/

@[extern "rs_multi_borrow_sum"]
opaque multiBorrowSum : @& Array Nat → Nat

@[extern "rs_list_to_array_via_push"]
opaque listToArrayViaPush : @& List Nat → Array Nat

@[extern "rs_borrow_to_owned"]
opaque borrowToOwned : @& Nat → Nat

@[extern "rs_make_empty_array"]
opaque makeEmptyArray : Unit → Array Nat

@[extern "rs_make_empty_list"]
opaque makeEmptyList : Unit → List Nat

@[extern "rs_make_empty_bytearray"]
opaque makeEmptyByteArray : Unit → ByteArray

@[extern "rs_make_empty_string"]
opaque makeEmptyString : Unit → String

@[extern "rs_nat_max_scalar"]
opaque natMaxScalar : Unit → Nat

@[extern "rs_nat_min_heap"]
opaque natMinHeap : Unit → Nat

@[extern "rs_external_all_fields"]
opaque externalAllFields : @& RustData → String

@[extern "rs_string_length"]
opaque stringLength : @& String → USize

/-! ## FFI declarations — memory management stress tests -/

@[extern "rs_alloc_drop_stress"]
opaque allocDropStress : Unit → UInt8

@[extern "rs_mutation_drop_stress"]
opaque mutationDropStress : Unit → UInt8

@[extern "rs_clone_drop_stress"]
opaque cloneDropStress : @& Array Nat → USize → USize

@[extern "rs_array_list_roundtrip"]
opaque arrayListRoundtrip : @& Array Nat → Array Nat

@[extern "rs_bytearray_copy_mutate"]
opaque byteArrayCopyMutate : ByteArray → ByteArray

/-! ## FFI declarations — in-place mutation tests -/

@[extern "rs_array_mut_ops"]
opaque arrayMutOps : Array Nat → Array Nat

@[extern "rs_bytearray_mut_ops"]
opaque bytearrayMutOps : ByteArray → ByteArray

@[extern "rs_string_mut_ops"]
opaque stringMutOps : String → @& String → String

@[extern "rs_external_set_x"]
opaque externalSetX : RustData → UInt64 → RustData

@[extern "rs_external_lifecycle"]
opaque externalLifecycle : UInt64 → UInt64 → @& String → UInt64 → String

/-! ## FFI declarations — persistent object tests -/

@[extern "rs_is_persistent"]
opaque isPersistent : @& Nat → UInt8

@[extern "rs_read_persistent_nat"]
opaque readPersistentNat : @& Nat → Nat

@[extern "rs_read_persistent_point"]
opaque readPersistentPoint : @& Point → Nat

@[extern "rs_read_persistent_array"]
opaque readPersistentArray : @& Array Nat → Nat

@[extern "rs_read_persistent_string"]
opaque readPersistentString : @& String → Nat

@[extern "rs_drop_persistent_nat"]
opaque dropPersistentNat : Nat → Nat

/-! ## LeanShared — multi-threaded refcounting tests -/

@[extern "rs_shared_parallel_read"]
opaque sharedParallelRead : @& Array Nat → USize → Nat

@[extern "rs_shared_parallel_nat"]
opaque sharedParallelNat : @& Nat → USize → Nat

@[extern "rs_shared_parallel_string"]
opaque sharedParallelString : @& String → USize → Nat

@[extern "rs_shared_contention_stress"]
opaque sharedContentionStress : @& Array Nat → USize → USize → Nat

@[extern "rs_shared_into_owned"]
opaque sharedIntoOwned : @& Nat → Nat

@[extern "rs_shared_parallel_point"]
opaque sharedParallelPoint : @& Point → USize → Nat

@[extern "rs_shared_persistent_nat"]
opaque sharedPersistentNat : @& Nat → USize → Nat

/-! ## Persistent module-level values -/
-- These become persistent (m_rc == 0) after module initialization.

private def persistentNat : Nat := 42
private def persistentLargeNat : Nat := 2^128
private def persistentPoint : Point := ⟨10, 20⟩
private def persistentArray : Array Nat := #[1, 2, 3, 4, 5]
private def persistentString : String := "hello persistent"

/-! ## Borrowed roundtrip tests — types without property test generators -/

def borrowedRoundtripTests : TestSeq :=
  test "Bool true" (roundtripBool true == true) ++
  test "Bool false" (roundtripBool false == false) ++
  test "UInt32 max" (roundtripUInt32 0xFFFFFFFF == 0xFFFFFFFF) ++
  test "UInt64 max" (roundtripUInt64 0xFFFFFFFFFFFFFFFF == 0xFFFFFFFFFFFFFFFF) ++
  test "Float 3.14" (roundtripFloat 3.14 == 3.14) ++
  test "Float -1.5" (roundtripFloat (-1.5) == -1.5) ++
  test "Float32 3.14" (roundtripFloat32 3.14 == 3.14) ++
  test "Array UInt32 [0, max]" (roundtripArrayUInt32 #[0, 0xFFFFFFFF] == #[0, 0xFFFFFFFF]) ++
  test "Array UInt64 [0, max]" (roundtripArrayUInt64 #[0, 0xFFFFFFFFFFFFFFFF] == #[0, 0xFFFFFFFFFFFFFFFF]) ++
  test "Array Float [1.5, 2.5]" (roundtripArrayFloat #[1.5, 2.5] == #[1.5, 2.5]) ++
  test "Array Float32 [1.5, 2.5]" (roundtripArrayFloat32 #[1.5, 2.5] == #[1.5, 2.5]) ++
  test "Nested array [[]]" (roundtripNestedArray #[#[]] == #[#[]]) ++
  test "Nested list [[]]" (roundtripNestedList [[]] == [[]]) ++
  test "ScalarStruct zeros" (roundtripScalarStruct ⟨0, 0, 0, 0⟩ == ⟨0, 0, 0, 0⟩) ++
  test "ScalarStruct max" (roundtripScalarStruct ⟨100, 0xFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF⟩ == ⟨100, 0xFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF⟩) ++
  test "ExtScalarStruct zeros" (show Bool from roundtripExtScalarStruct ⟨0, 0, 0, 0, 0, 0.0, 0.0⟩ == ⟨0, 0, 0, 0, 0, 0.0, 0.0⟩) ++
  test "ExtScalarStruct max" (show Bool from roundtripExtScalarStruct ⟨100, 0xFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 1.0, 1.0⟩ == ⟨100, 0xFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 1.0, 1.0⟩) ++
  test "USizeStruct zeros" (roundtripUSizeStruct ⟨0, 0, 0⟩ == ⟨0, 0, 0⟩) ++
  test "USizeStruct mixed" (roundtripUSizeStruct ⟨42, 99, 255⟩ == ⟨42, 99, 255⟩) ++
  test "External all fields" (externalAllFields (mkRustData 42 99 "hello") == "42:99:hello") ++
  test "External all fields zeros" (externalAllFields (mkRustData 0 0 "") == "0:0:") ++
  test "External large u64" (rustDataGetX (mkRustData 0xFFFFFFFFFFFFFFFF 0 "test") == 0xFFFFFFFFFFFFFFFF) ++
  test "Except error_string" (show Bool from
    match exceptErrorString "boom" with | .error s => s == "boom" | .ok _ => false) ++
  test "IOResult ok" (show Bool from
    match ioResultOkNat 42 with | .ok val _ => val == 42 | .error _ _ => false) ++
  test "IOResult error" (show Bool from
    match ioResultErrorString "oops" with | .error _ _ => true | .ok _ _ => false) ++
  test "Borrowed result chain" (borrowedResultChain (#[1, 2], #[3, 4]) == 10) ++
  test "Borrowed except ok" (borrowedExceptValue (.ok 42) == 42) ++
  test "Borrowed except error" (borrowedExceptValue (.error "hello") == 5) ++
  test "String length unicode" (stringLength "héllo" == 5)

/-! ## Edge cases — Nat boundaries and empty collections -/

def edgeCaseTests : TestSeq :=
  let natBoundaries : List Nat := [0, 1, 255, 256, 65535, 65536, (2^32 - 1), 2^32,
    (2^63 - 1), 2^63, (2^64 - 1), 2^64, 2^64 + 1, 2^128, 2^256]
  natBoundaries.foldl (init := .done) fun acc n =>
    acc ++ .individualIO s!"Nat {n}" none (do
      let rt := roundtripNat n
      pure (rt == n, 0, 0, if rt == n then none else some s!"got {rt}")) .done ++
  test "Make empty array" (makeEmptyArray () == #[]) ++
  test "Make empty list" (makeEmptyList () == []) ++
  test "Make empty bytearray" (makeEmptyByteArray () == ⟨#[]⟩) ++
  test "Make empty string" (makeEmptyString () == "") ++
  test "Nat max scalar" (natMaxScalar () == (2^63 - 1)) ++
  test "Nat min heap" (natMinHeap () == 2^63)

/-! ## Owned argument tests — patterns not covered by property tests -/

def ownedArgTests : TestSeq :=
  test "Drop and replace" (ownedDropAndReplace "hello" == "replaced:5") ++
  test "Merge 3 owned lists" (ownedMergeLists [1, 2] [3] [4, 5] == [1, 2, 3, 4, 5]) ++
  test "Merge 3 empty lists" (ownedMergeLists [] [] [] == []) ++
  test "Reverse bytearray" (ownedReverseByteArray ⟨#[1, 2, 3]⟩ == ⟨#[3, 2, 1]⟩) ++
  test "Reverse empty bytearray" (ownedReverseByteArray ⟨#[]⟩ == ⟨#[]⟩) ++
  test "IOResult ok value" (ownedIOResultValue (ioResultOkNat 42) == 42) ++
  test "IOResult error value" (ownedIOResultValue (ioResultErrorString "oops") == 0)

/-! ## In-place mutation tests -/

def mutationTests : TestSeq :=
  test "ByteArray copy mutate" (byteArrayCopyMutate ⟨#[1, 2, 3]⟩ == ⟨#[255, 2, 3]⟩) ++
  test "External lifecycle" (externalLifecycle 10 20 "hi" 99 == "10:20:hi/99:20:hi")

/-! ## Chained owned FFI — each type: create → owned roundtrip → owned roundtrip → check -/

def chainedTests : TestSeq :=
  let arr := ownedArrayNatRoundtrip (ownedArrayNatRoundtrip #[1, 2, 3])
  test "Array" (arr == #[1, 2, 3]) ++
  let ba := ownedByteArrayRoundtrip (ownedByteArrayRoundtrip ⟨#[10, 20, 30]⟩)
  test "ByteArray" (ba == ⟨#[10, 20, 30]⟩) ++
  let s := ownedStringRoundtrip (ownedStringRoundtrip "hello")
  test "String" (s == "hello") ++
  let obj := externalSetX (externalSetX (mkRustData 10 20 "hi") 99) 42
  test "External" (rustDataGetX obj == 42 && rustDataGetY obj == 20)

/-! ## Memory management stress tests (Valgrind targets) -/
-- These functions allocate and drop objects entirely in Rust without returning
-- them to Lean. Valgrind detects leaks, double-frees, or use-after-free.

def memoryTests : TestSeq :=
  test "Alloc/drop stress" (allocDropStress () == 1) ++
  test "Mutation/drop stress" (mutationDropStress () == 1) ++
  test "Clone/drop stress" (cloneDropStress #[1, 2, 3] 100 == 300)

/-! ## Persistent object tests -/
-- Module-level `def` values become persistent after initialization (m_rc == 0).
-- These tests verify that borrowed access to persistent objects works correctly,
-- and that LeanOwned::drop is a no-op for persistent data.

def persistentTests : TestSeq :=
  -- Reading persistent values as borrowed (no ref counting)
  test "Read persistent Nat" (readPersistentNat persistentNat == 42) ++
  test "Read persistent large Nat" (readPersistentNat persistentLargeNat == 2^128) ++
  test "Read persistent Point" (readPersistentPoint persistentPoint == 30) ++
  test "Read persistent Array" (readPersistentArray persistentArray == 15) ++
  test "Read persistent String" (readPersistentString persistentString == 16) ++
  -- Dropping persistent values via owned arg (lean_dec is no-op for m_rc == 0)
  test "Drop persistent Nat" (dropPersistentNat persistentNat == 42) ++
  test "Drop persistent large Nat" (dropPersistentNat persistentLargeNat == 2^128) ++
  -- Read the same persistent value multiple times (verifies it wasn't freed)
  test "Persistent Nat stable" (readPersistentNat persistentNat == 42) ++
  test "Persistent Array stable" (readPersistentArray persistentArray == 15)

/-! ## Property-based tests -/

/-! ## Suite organization -/
-- Tests are grouped by what they exercise:
--   "borrowed"   — @& args, no ref counting in Rust
--   "owned"      — lean_obj_arg, LeanOwned Drop calls lean_dec
--   "persistent" — m_rc == 0 objects (compact regions, module-level defs)
--   "property"   — randomized property-based tests (SlimCheck)

public def borrowedSuite : List TestSeq := [
  group "Roundtrip" borrowedRoundtripTests,
  group "Edge cases" edgeCaseTests,
]

public def ownedSuite : List TestSeq := [
  group "Drop" ownedArgTests,
  group "In-place mutation" mutationTests,
  group "Chained FFI" chainedTests,
  group "Memory management" memoryTests,
]

public def persistentSuite : List TestSeq := [
  persistentTests,
]

public def propertySuite : List TestSeq := [
  group "Borrowed roundtrip" (
    checkIO "Nat" (∀ n : Nat, roundtripNat n == n) ++
    checkIO "String" (∀ s : String, roundtripString s == s) ++
    checkIO "List Nat" (∀ xs : List Nat, roundtripListNat xs == xs) ++
    checkIO "Array Nat" (∀ arr : Array Nat, roundtripArrayNat arr == arr) ++
    checkIO "ByteArray" (∀ ba : ByteArray, roundtripByteArray ba == ba) ++
    checkIO "Option Nat" (∀ o : Option Nat, roundtripOptionNat o == o) ++
    checkIO "Point" (∀ p : Point, roundtripPoint p == p) ++
    checkIO "NatTree" (∀ t : NatTree, roundtripNatTree t == t) ++
    checkIO "Except" (∀ e : Except String Nat, show Bool from roundtripExceptStringNat e == e) ++
    checkIO "UInt32" (∀ n : UInt32, roundtripUInt32 n == n) ++
    checkIO "UInt64" (∀ n : UInt64, roundtripUInt64 n == n) ++
    checkIO "USize" (∀ n : USize, roundtripUSize n == n) ++
    checkIO "String from bytes" (∀ s : String, roundtripStringFromBytes s == s) ++
    checkIO "Array push" (∀ arr : Array Nat, roundtripArrayPush arr == arr) ++
    checkIO "Array list roundtrip" (∀ arr : Array Nat, arrayListRoundtrip arr == arr) ++
    checkIO "Nested Array" (∀ arr : Array (Array Nat), roundtripNestedArray arr == arr) ++
    checkIO "Nested List" (∀ xs : List (List Nat), roundtripNestedList xs == xs) ++
    checkIO "Prod swap" (∀ p : Nat × Nat, prodSwap p == (p.2, p.1)) ++
    checkIO "Borrow to owned" (∀ n : Nat, borrowToOwned n == n) ++
    checkIO "String length" (∀ s : String, (stringLength s).toNat == s.length) ++
    checkIO "Option unwrap" (∀ o : Option Nat, optionUnwrapOrZero o == o.getD 0) ++
    checkIO "Except map ok" (∀ e : Except String Nat,
      exceptMapOk e == match e with | .ok n => n + 1 | .error _ => 0) ++
    checkIO "Multi borrow sum" (∀ arr : Array Nat, multiBorrowSum arr == arr.toList.foldl (· + ·) 0)),
  group "Owned Drop" (
    checkIO "Nat" (∀ n : Nat, ownedNatRoundtrip n == n) ++
    checkIO "String" (∀ s : String, ownedStringRoundtrip s == s) ++
    checkIO "Array Nat" (∀ arr : Array Nat, ownedArrayNatRoundtrip arr == arr) ++
    checkIO "List Nat" (∀ xs : List Nat, ownedListNatRoundtrip xs == xs) ++
    checkIO "ByteArray" (∀ ba : ByteArray, ownedByteArrayRoundtrip ba == ba) ++
    checkIO "Option Nat" (∀ o : Option Nat, ownedOptionRoundtrip o == o) ++
    checkIO "Prod" (∀ p : Nat × Nat, ownedProdRoundtrip p == p) ++
    checkIO "Prod multiply" (∀ p : Nat × Nat, ownedProdMultiply p == p.1 * p.2) ++
    checkIO "Option square" (∀ o : Option Nat, ownedOptionSquare o == match o with | some n => n * n | none => 0) ++
    checkIO "Except transform" (∀ e : Except String Nat,
      ownedExceptTransform e == match e with | .ok n => 2 * n | .error s => s.utf8ByteSize) ++
    checkIO "Append nat" (∀ arr : Array Nat, ∀ n : Nat, ownedAppendNat arr n == arr.push n) ++
    checkIO "Point sum" (∀ p : Point, ownedPointSum p == p.x + p.y) ++
    checkIO "Scalar sum" (∀ s : ScalarStruct,
      ownedScalarSum s == s.u8val.toUInt64 + s.u32val.toUInt64 + s.u64val)),
  group "Clone + Drop" (
    checkIO "Except" (∀ e : Except String Nat,
      cloneExcept e == match e with | .ok n => 2 * n | .error s => 2 * s.utf8ByteSize) ++
    checkIO "List" (∀ xs : List Nat, cloneList xs == 2 * xs.length) ++
    checkIO "ByteArray" (∀ ba : ByteArray, cloneByteArray ba == 2 * ba.size) ++
    checkIO "Option" (∀ o : Option Nat,
      cloneOption o == match o with | some n => 2 * n | none => 0) ++
    checkIO "Prod" (∀ p : Nat × Nat, cloneProd p == 2 * (p.1 + p.2)) ++
    checkIO "Array len sum" (∀ arr : Array Nat, (cloneArrayLenSum arr).toNat == 2 * arr.size) ++
    checkIO "String len sum" (∀ s : String, (cloneStringLenSum s).toNat == 2 * s.utf8ByteSize)),
  group "Misc" (
    checkIO "Array data sum" (∀ arr : Array Nat, arrayDataSum arr == arr.toList.foldl (· + ·) 0) ++
    checkIO "List to array via push" (∀ xs : List Nat, listToArrayViaPush xs == xs.toArray)),
]

/-! ## LeanShared (multi-threaded) tests -/

def sharedTests : TestSeq :=
  -- Parallel read: N threads all read same array, sum should be N * element_sum
  test "Shared parallel read 4 threads" (sharedParallelRead #[1, 2, 3] 4 == 24) ++
  test "Shared parallel read 8 threads" (sharedParallelRead #[10, 20] 8 == 240) ++
  test "Shared parallel read empty" (sharedParallelRead #[] 4 == 0) ++
  test "Shared parallel read 1 thread" (sharedParallelRead #[42] 1 == 42) ++
  -- Parallel Nat: all threads should read same value
  test "Shared parallel Nat 42" (sharedParallelNat 42 4 == 42) ++
  test "Shared parallel Nat large" (sharedParallelNat (2^64) 4 == 2^64) ++
  -- Parallel String: sum of byte_len across threads
  test "Shared parallel String" (sharedParallelString "hello" 4 == 20) ++
  test "Shared parallel String empty" (sharedParallelString "" 4 == 0) ++
  -- Contention stress: rapid clone/drop from many threads
  test "Shared contention 4 threads 100 clones" (sharedContentionStress #[1, 2, 3] 4 100 == 12) ++
  test "Shared contention 8 threads 50 clones" (sharedContentionStress #[10] 8 50 == 8) ++
  -- into_owned: unwrap MT-marked LeanShared back to LeanOwned
  test "Shared into_owned 42" (sharedIntoOwned 42 == 42) ++
  test "Shared into_owned large" (sharedIntoOwned (2^128) == 2^128) ++
  -- Constructor types: lean_mark_mt walks object graph
  test "Shared parallel Point 4 threads" (sharedParallelPoint ⟨10, 20⟩ 4 == 120) ++
  test "Shared parallel Point 1 thread" (sharedParallelPoint ⟨3, 7⟩ 1 == 10) ++
  test "Shared parallel Point zeros" (sharedParallelPoint ⟨0, 0⟩ 4 == 0) ++
  -- Persistent objects: lean_mark_mt skipped, refcount ops are no-ops
  test "Shared persistent Nat" (sharedPersistentNat persistentNat 4 == 42) ++
  test "Shared persistent large Nat" (sharedPersistentNat persistentLargeNat 4 == 2^128)

public def sharedSuite : List TestSeq := [
  group "LeanShared MT" sharedTests,
]

end Tests.FFI
