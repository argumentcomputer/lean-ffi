/-
  Generators and test types for property-based FFI roundtrip tests.
-/
module

public import LSpec

public section

open LSpec SlimCheck Gen

/-! ## Basic type generators -/

/-- Generate Nats across the full range: small, medium, large, and huge -/
def genNat : Gen Nat := do
  let choice ← choose Nat 0 100
  if choice < 50 then
    -- 50%: small nats (0-1000)
    choose Nat 0 1000
  else if choice < 75 then
    -- 25%: medium nats (up to 2^32)
    choose Nat 0 (2^32)
  else if choice < 90 then
    -- 15%: large nats (up to 2^64)
    choose Nat 0 (2^64)
  else
    -- 10%: huge nats (up to 2^256)
    choose Nat 0 (2^256)

def genSmallNat : Gen Nat := choose Nat 0 1000

def genString : Gen String := do
  let len ← choose Nat 0 100
  let chars ← Gen.listOf (choose Nat 32 126 >>= fun n => pure (Char.ofNat n))
  pure (String.ofList (chars.take len))

def genListNat : Gen (List Nat) := do
  let len ← choose Nat 0 20
  let mut result := []
  for _ in [:len] do
    result := (← genSmallNat) :: result
  pure result.reverse

def genArrayNat : Gen (Array Nat) := do
  let list ← genListNat
  pure list.toArray

def genByteArray : Gen ByteArray := do
  let len ← choose Nat 0 100
  let mut bytes := ByteArray.emptyWithCapacity len
  for _ in [:len] do
    let b ← choose Nat 0 255
    bytes := bytes.push b.toUInt8
  pure bytes

def genBool : Gen Bool := choose Bool .false true

def genOptionNat : Gen (Option Nat) := do
  let b ← genBool
  if b then pure none else some <$> genSmallNat

/-! ## Test struct/inductive types -/

/-- A simple 2D point struct for FFI testing -/
structure Point where
  x : Nat
  y : Nat
deriving Repr, BEq, DecidableEq, Inhabited

def genPoint : Gen Point := do
  let x ← genSmallNat
  let y ← genSmallNat
  pure ⟨x, y⟩

/-- A simple binary tree of Nats for FFI testing -/
inductive NatTree where
  | leaf : Nat → NatTree
  | node : NatTree → NatTree → NatTree
deriving Repr, BEq, DecidableEq, Inhabited

/-- Generate a random NatTree with bounded depth -/
def genNatTree : Nat → Gen NatTree
  | 0 => do
    let n ← genSmallNat
    pure (.leaf n)
  | maxDepth + 1 => do
    let choice ← choose Nat 0 2
    if choice == 0 then
      let n ← genSmallNat
      pure (.leaf n)
    else
      let left ← genNatTree maxDepth
      let right ← genNatTree maxDepth
      pure (.node left right)

/-- A structure with mixed object and scalar fields for FFI testing.
    Layout: 1 object field (obj : Nat), then scalar fields (u8val, u32val, u64val). -/
structure ScalarStruct where
  obj : Nat
  u8val : UInt8
  u32val : UInt32
  u64val : UInt64
deriving Repr, BEq, DecidableEq, Inhabited

/-- Extended scalar struct with all scalar types including u16, Float, and Float32.
    Layout: 1 object field (obj : Nat), then scalar fields. -/
structure ExtScalarStruct where
  obj : Nat
  u8val : UInt8
  u16val : UInt16
  u32val : UInt32
  u64val : UInt64
  fval : Float
  f32val : Float32
deriving Repr, BEq, Inhabited

/-- Structure with a USize scalar field.
    Layout: 1 object field (obj : Nat), then USize, then UInt8. -/
structure USizeStruct where
  obj : Nat
  uval : USize
  u8val : UInt8
deriving Repr, BEq, DecidableEq, Inhabited

/-! ## Shrinkable instances -/

instance : Shrinkable Nat where
  shrink n := if n == 0 then [] else [n / 2]

instance : Shrinkable (List Nat) where
  shrink xs := match xs with
    | [] => []
    | _ :: tail => [tail]

instance : Shrinkable (Array Nat) where
  shrink arr := if arr.isEmpty then [] else [arr.pop]

instance : Repr ByteArray where
  reprPrec ba _ := s!"ByteArray#{ba.toList}"

instance : Shrinkable ByteArray where
  shrink ba := if ba.isEmpty then [] else [ba.extract 0 (ba.size - 1)]

instance : Shrinkable String where
  shrink s := if s.isEmpty then [] else [s.dropEnd 1 |>.toString]

instance : Shrinkable Point where
  shrink p := if p.x == 0 && p.y == 0 then [] else [⟨p.x / 2, p.y / 2⟩]

instance : Shrinkable NatTree where
  shrink t := match t with
    | .leaf n => if n == 0 then [] else [.leaf (n / 2)]
    | .node l r => [l, r]

instance : Shrinkable (Option Nat) where
  shrink o := match o with
    | none => []
    | some n => none :: (Shrinkable.shrink n |>.map some)

/-! ## SampleableExt instances -/

instance : SampleableExt Nat := SampleableExt.mkSelfContained genNat
instance : SampleableExt (List Nat) := SampleableExt.mkSelfContained genListNat
instance : SampleableExt (Array Nat) := SampleableExt.mkSelfContained genArrayNat
instance : SampleableExt ByteArray := SampleableExt.mkSelfContained genByteArray
instance : SampleableExt String := SampleableExt.mkSelfContained genString
instance : SampleableExt Point := SampleableExt.mkSelfContained genPoint
instance : SampleableExt NatTree := SampleableExt.mkSelfContained (genNatTree 4)
instance : SampleableExt (Option Nat) := SampleableExt.mkSelfContained genOptionNat

end
