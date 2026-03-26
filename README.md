# lean-ffi

A Rust library that wraps low-level bindings to the
[`lean.h`](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h)
Lean C library with a high-level Rust API for safe and ergonomic FFI from Lean
to Rust. This allows the user to focus on the actual Rust logic rather than
manually manipulating pointers and keeping track of Lean reference counts.

The Rust bindings are auto-generated with
[`rust-bindgen`](https://github.com/rust-lang/rust-bindgen). Bindgen runs in
`build.rs` and generates unsafe Rust functions that link to `lean.h`. This
module can be found at `target/release/build/lean-ffi-<hash>/out/lean.rs` after
running `cargo build --release`.

## Background: Lean Ownership Model

In Lean's C API, a **reference** is a `lean_object*` pointer to the header of a
heap-allocated object. References in Lean can either be **owned** or
**borrowed**.

An **owned reference**, signified by `lean_obj_arg` in C, uses reference
counting via the `int m_rc` field of the `lean_object` to determine when to free
the underlying object. Before a new reference to the object is created, the Lean
compiler inserts a `lean_inc` call to increment the ref count. When the
reference goes out of scope, the Lean compiler inserts a `lean_dec` call to
decrement the ref count. When `m_rc` reaches 0, the Lean runtime frees the
object.

A **borrowed reference**, signified by `@&` in Lean and `b_lean_obj_arg` in C,
inherits the reference count of a surrounding owned reference, and is assumed to
be kept alive as long as its parent. This enables the borrowed reference to
dispense with reference counting altogether as it will get dropped when going
out of scope.

> [!NOTE] A `lean_object*` can also refer to a tagged scalar value encoded as a
> pointer-sized data type, where the low bit (tag) of the pointer is set to 1.
> In that case it would not be called a reference.

## `lean-ffi` Rust API

In order to handle Lean reference counting gracefully in Rust, we use the
following types:

- **`LeanOwned`** - An owned reference to a Lean object with RAII semantics.
  Corresponds to `lean_obj_arg` (input) and `lean_obj_res` (output) in the C
  FFI.
  - The `Clone` implementation calls `lean_inc` and returns a new `LeanOwned`
    reference to the same object. `Copy` is not implemented.
  - The `Drop` implementation calls `lean_dec` automatically on scope exit.
  - Passing or assigning a `LeanOwned` **moves** it (transferring the
    `lean_dec`); use `self.clone()` to create a second owned reference via
    `lean_inc`.
  - `self.into_raw()` consumes the wrapper **without** calling `lean_dec`, for
    passing ownership to Lean C API functions that take `lean_obj_arg` (which
    will `lean_dec` internally). Not needed for returning values from
    `extern "C"` functions — returning `LeanOwned` directly works because Rust
    does not call `Drop` on return values.
  - Tagged scalar values (bit 0 set — small `Nat`, `Bool`, etc.) and persistent
    objects (`m_rc == 0`) skip refcount operations entirely.

- **`LeanBorrowed<'a>`** — A borrowed reference. Corresponds to `b_lean_obj_arg`
  in the C FFI. Used when Lean declares a parameter with `@&`.
  - The `Copy` and `Clone` implementations perform a trivial bitwise copy.
    Neither `Clone` nor `Drop` modify the reference count.
  - The lifetime `'a` ties the borrowed reference to the source reference's
    scope, preventing use-after-free.
  - Call `self.to_owned_ref()` to promote to `LeanOwned` (calls `lean_inc`).
  - Note: The `b_lean_obj_res` type is used when returning a borrowed reference
    in C, but returning it and `LeanBorrowed` are only used internally as Lean
    expects owned references at the FFI boundary.

- **`LeanShared`** — A thread-safe owned reference. Wraps `LeanOwned` after
  calling `lean_mark_mt` on the object graph, which transitions all reachable
  objects to multi-threaded mode with atomic refcounting. Implements
  `Send + Sync`. Use `borrow()` to get a `LeanBorrowed<'_>` for reading,
  `into_owned()` to unwrap back to `LeanOwned`.

- **`LeanRef`** — Trait implemented by `LeanOwned`, `LeanBorrowed`, and
  `LeanShared`, providing shared read-only operations like `self.as_raw()`,
  `self.is_scalar()`, `self.tag()`, and unboxing methods.

All reference types are safe for persistent objects and compact memory regions
(`m_rc == 0`) — `lean_inc_ref()` and `lean_dec_ref()` are no-ops when
`m_rc == 0`.

### Domain Types

Domain types wrap the ownership types to provide type safety at FFI boundaries.
Built-in domain types include `LeanArray<R>`, `LeanString<R>`, `LeanCtor<R>`,
`LeanList<R>`, `LeanOption<R>`, `LeanExcept<R>`, `LeanIOResult<R>`,
`LeanProd<R>`, `LeanNat<R>`, `LeanBool<R>`, `LeanByteArray<R>`, and
`LeanExternal<T, R>`.

#### Naming convention

Domain types are prefixed with `Lean` to distinguish them from Lean-side type
names and to match the built-in types. For example, a Lean `Point` structure
becomes `LeanPoint` in Rust.

#### Defining custom domain types

Use the `lean_domain_type!` macro to define newtypes for your Lean types:

```rust
lean_ffi::lean_domain_type! {
    /// Lean `Point` — structure Point where x : Nat; y : Nat
    LeanPoint;
    /// Lean `PutResponse` — structure PutResponse where message : String; hash : String
    LeanPutResponse;
}
```

This generates a `#[repr(transparent)]` wrapper with `Clone`, `Copy` for
`LeanBorrowed`, `self.inner()`, `self.as_raw()`, `self.into_raw()`, and `From`
impls. You can then add accessor methods — readers are generic over `R: LeanRef`
(work on both owned and borrowed), constructors return `LeanOwned`:

```rust
impl<R: LeanRef> LeanPutResponse<R> {
    pub fn message(&self) -> LeanBorrowed<'_> {
        self.as_ctor().get(0)  // borrowed ref into the object, no lean_inc
    }
    pub fn hash(&self) -> LeanBorrowed<'_> {
        self.as_ctor().get(1)
    }
}

impl LeanPutResponse<LeanOwned> {
    pub fn mk(message: &str, hash: &str) -> Self {
        let ctor = LeanCtor::alloc(0, 2, 0);
        ctor.set(0, LeanString::new(message));
        ctor.set(1, LeanString::new(hash));
        Self::new(ctor.into())
    }
}
```

### Inductive types and field layout

Extra care must be taken when dealing with
[inductive types](https://lean-lang.org/doc/reference/latest/The-Type-System/Inductive-Types/#run-time-inductives)
as the runtime memory layout of constructor fields may not match the declaration
order in Lean. Fields are reordered into three groups:

1. Non-scalar fields (`lean_object*`), in declaration order
2. `USize` fields, in declaration order
3. Other scalar fields, in decreasing order by size, then declaration order
   within each size

This means a structure like

```lean
structure MyStruct where
  u8val : UInt8
  obj : Nat
  u32val : UInt32
  u64val : UInt64
```

would be laid out as `[obj, u64val, u32val, u8val]` at runtime. Trivial wrapper
types (e.g. `Char` wraps `UInt32`) count as their underlying scalar type.

A constructor's memory looks like:

```
[header (8B)] [object fields (8B each)] [USize fields (8B each)] [scalar data area]
```

Object fields and USize fields each occupy 8-byte slots. The scalar data area is
a flat region of bytes containing all remaining scalar field values, packed by
descending size. For `MyStruct` (1 object field, 0 USize fields, 13 scalar
bytes):

- `u64val` occupies bytes 0–7 of the scalar area
- `u32val` occupies bytes 8–11
- `u8val` occupies byte 12

Use `LeanCtor` to access fields at the correct positions. Scalar getters and
setters take `(num_slots, byte_offset)` — `num_slots` is the total number of
8-byte slots (object fields + USize fields) preceding the scalar data area, and
`byte_offset` is the position of the field within that area.

```rust
impl<R: LeanRef> LeanScalarStruct<R> {
    pub fn obj(&self) -> LeanBorrowed<'_> { self.as_ctor().get(0) }
    pub fn u64val(&self) -> u64 { self.as_ctor().get_u64(1, 0) }
    pub fn u32val(&self) -> u32 { self.as_ctor().get_u32(1, 8) }
    pub fn u8val(&self) -> u8  { self.as_ctor().get_u8(1, 12) }
}

impl LeanScalarStruct<LeanOwned> {
    pub fn mk(obj: LeanNat<LeanOwned>, u64val: u64, u32val: u32, u8val: u8) -> Self {
        let ctor = LeanCtor::alloc(0, 1, 13); // tag 0, 1 obj field, 13 scalar bytes
        ctor.set(0, obj);                // object field 0
        ctor.set_u64(1, 0, u64val);      // 1 slot before scalars, byte 0
        ctor.set_u32(1, 8, u32val);      // 1 slot before scalars, byte 8
        ctor.set_u8(1, 12, u8val);       // 1 slot before scalars, byte 12
        Self::new(ctor.into())
    }
}
```

### External objects (`LeanExternal<T, R>`)

External objects let you store arbitrary Rust data inside a Lean object. Lean
sees an opaque type; Rust controls allocation, access, mutation, and cleanup.

**Register** an external class exactly once, using `OnceLock` or `LazyLock`.

`ExternalClass::register()` calls `lean_register_external_class`, which
allocates a class descriptor with two function pointers: a **finalizer** called
when the object's refcount reaches zero to free the Rust data, and a **foreach**
callback that `lean_mark_persistent` and `lean_mark_mt` use to traverse any
embedded `lean_object*` pointers (usually a no-op for pure Rust data).

`ExternalClass::register_with_drop::<T>()` generates a finalizer that calls
`drop(Box::from_raw(ptr.cast::<T>()))` and a no-op foreach — sufficient for any
Rust type that doesn't hold Lean objects.

Registration must happen exactly once per type. `LazyLock` (or `OnceLock`)
ensures thread-safe one-time initialization, storing the returned
`ExternalClass` in a `static` for reuse across all allocations:

```rust
use std::sync::LazyLock;
use lean_ffi::object::{ExternalClass, LeanExternal, LeanOwned, LeanBorrowed};

struct Hasher { state: Vec<u8> }

static HASHER_CLASS: LazyLock<ExternalClass> =
    LazyLock::new(ExternalClass::register_with_drop::<Hasher>);
```

**Create** — `LeanExternal::alloc()` boxes the value and returns an owned
reference to the external object:

```rust
// Lean: @[extern "rs_hasher_new"] opaque Hasher.new : Unit → Hasher
#[unsafe(no_mangle)]
extern "C" fn rs_hasher_new(_unit: LeanOwned) -> LeanExternal<Hasher, LeanOwned> {
    LeanExternal::alloc(&HASHER_CLASS, Hasher { state: Vec::new() })
}
```

**Read** — `self.get()` borrows the stored `&T`. Works on both owned and
borrowed references:

```rust
// Lean: @[extern "rs_hasher_bytes"] opaque Hasher.bytes : @& Hasher → ByteArray
#[unsafe(no_mangle)]
extern "C" fn rs_hasher_bytes(
    h: LeanExternal<Hasher, LeanBorrowed<'_>>,  // @& → borrowed
) -> LeanByteArray<LeanOwned> {
    LeanByteArray::from_bytes(&h.get().state)  // &Hasher — no clone, no refcount change
}
```

**Update** — `self.get_mut()` returns `Option<&mut T>`, which is `Some` when the
object is exclusively owned (`m_rc == 1`). This enables in-place mutation
without allocating a new external object. When shared `self.get_mut()` returns
`None` and instead clones into a new object on write.

```rust
// Lean: @[extern "rs_hasher_update"] opaque Hasher.update : Hasher → @& ByteArray → Hasher
#[unsafe(no_mangle)]
extern "C" fn rs_hasher_update(
    mut h: LeanExternal<Hasher, LeanOwned>,
    input: LeanByteArray<LeanBorrowed<'_>>,
) -> LeanExternal<Hasher, LeanOwned> {
    if let Some(state) = h.get_mut() {
        state.state.extend_from_slice(input.as_bytes());  // mutate in place
        h
    } else {
        // shared — clone and allocate a new external object
        let mut new_state = h.get().clone();
        new_state.state.extend_from_slice(input.as_bytes());
        LeanExternal::alloc(&HASHER_CLASS, new_state)
    }
}
```

**Delete** — follows the same ownership rules as other domain types:

- `LeanExternal<T, LeanOwned>` — `Drop` calls `lean_dec`. When the refcount
  reaches zero, Lean calls the class finalizer, which (via `register_with_drop`)
  runs `drop(Box::from_raw(ptr))` to free the Rust value.
- `LeanExternal<T, LeanBorrowed<'_>>` — no refcount changes, no cleanup. Use for
  `@&` parameters.
- Converting to `LeanOwned` (e.g. to store in a ctor field): call `.into()`.

### FFI function signatures

Use domain types in `extern "C"` function signatures. The ownership type
parameter tells Rust how to handle reference counting:

```rust
// Lean: @[extern "process"] def process (xs : @& Array Nat) (n : Nat) : Array Nat
#[no_mangle]
extern "C" fn process(
    xs: LeanArray<LeanBorrowed<'_>>,  // @& → borrowed, no lean_dec
    n: LeanNat<LeanOwned>,            // owned → lean_dec on drop
) -> LeanArray<LeanOwned> {           // returned to Lean, no drop
    // ...
}
```

More examples can be found in `src/test_ffi.rs` (Rust FFI implementations) and
`Tests/FFI.lean` (Lean declarations and tests), covering all domain types,
scalar field layouts, external objects, in-place mutation, and ownership
patterns.

### In-Place Mutation

Lean's runtime supports in-place mutation when an object is **exclusively
owned** (`m_rc == 1`, single-threaded mode). When shared, the object is copied
first. `LeanRef::is_exclusive()` exposes this check.

These methods consume `self` and return a (possibly new) object, mutating in
place when exclusive or copying first when shared:

#### `LeanArray`

| Method                    | C equivalent          | Description                                                        |
| ------------------------- | --------------------- | ------------------------------------------------------------------ |
| `self.set(&self, i, val)` | `lean_array_set_core` | Set element (asserts exclusive — use for freshly allocated arrays) |
| `self.uset(self, i, val)` | `lean_array_uset`     | Set element (copies if shared)                                     |
| `self.push(self, val)`    | `lean_array_push`     | Append an element                                                  |
| `self.pop(self)`          | `lean_array_pop`      | Remove the last element                                            |
| `self.uswap(self, i, j)`  | `lean_array_uswap`    | Swap elements at `i` and `j`                                       |

#### `LeanByteArray`

| Method                       | C equivalent                | Description                                                       |
| ---------------------------- | --------------------------- | ----------------------------------------------------------------- |
| `self.set_data(&self, data)` | `lean_sarray_cptr` + memcpy | Bulk write (asserts exclusive — use for freshly allocated arrays) |
| `self.uset(self, i, val)`    | `lean_byte_array_uset`      | Set byte (copies if shared)                                       |
| `self.push(self, val)`       | `lean_byte_array_push`      | Append a byte                                                     |
| `self.copy(self)`            | `lean_copy_byte_array`      | Deep copy into a new exclusive array                              |

#### `LeanString`

| Method                     | C equivalent         | Description                           |
| -------------------------- | -------------------- | ------------------------------------- |
| `self.push(self, c)`       | `lean_string_push`   | Append a UTF-32 character             |
| `self.append(self, other)` | `lean_string_append` | Concatenate another string (borrowed) |

`LeanExternal<T>` also supports in-place mutation via `get_mut()` — see the
**Update** section under [External objects](#external-objects-leanexternalt-r).

## Notes

### Rust panic behavior

By default, Rust uses stack unwinding for panics. If a panic occurs in a
Lean-to-Rust FFI function, the unwinding will try to cross the FFI boundary back
into Lean. This is
[undefined behavior](https://doc.rust-lang.org/stable/reference/panic.html#unwinding-across-ffi-boundaries).
To avoid this, configure Rust to abort on panic in `Cargo.toml`:

```toml
[profile.release]
panic = "abort"
```

### Enum FFI convention

Lean passes simple enums (inductives where all constructors have zero fields,
e.g. `DefKind`, `QuotKind`) as **raw unboxed tag values** (`0`, `1`, `2`, ...)
across the FFI boundary, not as `lean_box(tag)`. Use
`LeanOwned::from_enum_tag()` and `LeanRef::as_enum_tag()` for these.

### `lean_string_size` vs `lean_string_byte_size`

`lean_string_byte_size` returns the **total object memory size**
(`sizeof(lean_string_object) + capacity`), not the string data length. Use
`lean_string_size` instead, which returns `m_size` — the number of data bytes
including the NUL terminator. `LeanString` wraps these correctly:

- `self.byte_len()` — data bytes excluding NUL (`m_size - 1`)
- `self.length()` — UTF-8 character count (`m_length`)
- `self.as_str()` — view as `&str`

## References

- [Lean FFI documentation](https://lean-lang.org/doc/reference/latest/Run-Time-Code/#runtime)
- [`lean.h` C library](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h)
- [Counting Immutable Beans paper](https://arxiv.org/pdf/1908.05647)
- [Rust FFI guide](https://doc.rust-lang.org/nomicon/ffi.html)

## License

MIT or Apache 2.0
