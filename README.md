# lean-ffi

A Rust library that wraps low-level bindings to the
[`lean.h`](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h)
Lean C library with a high-level API for safe and ergonomic FFI from Lean to
Rust. This allows the user to focus on the actual Rust logic rather than manual
pointer manipulation and keeping track of Lean reference counts.

The raw Rust bindings are auto-generated with
[`rust-bindgen`](https://github.com/rust-lang/rust-bindgen). Bindgen runs in
`build.rs` and generates unsafe Rust functions that link to `lean.h`. This
module can be found at `target/release/build/lean-ffi-<hash>/out/lean.rs` after
running `cargo build --release`.

## Features

- **RAII refcounting** for `LeanOwned` owned references via Rust `Clone` and
  `Drop`
- **Lifetime bounds** for `LeanBorrowed` borrowed references to prevent
  use-after-free
- **Thread-safe shared references** via `LeanShared` (`lean_mark_mt` +
  `Send + Sync`).
- **Typed domain wrappers** e.g. `LeanArray`, `LeanString` etc. with safe Rust
  methods
- **`Nat` conversions** via `num-bigint`, handling tagged scalars and big-ints
- **`lean_inductive!` macro** for Lean `structure` / `inductive` types,
  generating easy accessor methods without manually tracking byte offsets
- **Safe external objects** via `ExternalClass::register_with_drop::<T>()` and
  borrow-bound `LeanExternal<T>::get(&self) -> &T`.

## Background: Lean Ownership Model

In Lean's C API, a **reference** is a `lean_object*` pointer to the header of a
heap-allocated object. References in Lean can either be **owned** or
**borrowed**.

An **owned reference** is a `lean_object*` that participates in reference
counting via the `int m_rc` field. Before a new reference to the object is
created, the Lean compiler inserts a `lean_inc` call to increment the ref count.
When the reference goes out of scope, the Lean compiler inserts a `lean_dec`
call to decrement the ref count. When `m_rc` reaches 0, the Lean runtime frees
the object. In C the conventional type alias for an owned reference is
`lean_obj_arg` for function parameters and `lean_obj_res` for return values.

A **borrowed reference**, signified by `@&` in a Lean function parameter, is a
`lean_object*` for which the compiler does not emit `lean_inc` or `lean_dec`
calls, relying on a surrounding owned reference to keep the object alive. This
is more efficient for cases when the object is known to outlive the borrowed
reference, e.g. reading a constructor field. In C the conventional type alias
for a borrowed reference is `b_lean_obj_arg` for function parameters and
`b_lean_obj_res` for return values.

> [!NOTE]
> A `lean_object*` can also refer to a tagged scalar value encoded as a
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

`lean_inductive!` wraps a Lean `structure` or `inductive` with its full
per-constructor layout. It emits a `#[repr(transparent)]` newtype, a
[`LeanCtorLayout`] impl whose `LAYOUTS` slice has one entry per constructor
(indexed by tag), and typed accessors that bounds-check against the current
variant's layout:

- `alloc(tag: u8)` — `lean_alloc_ctor` for a specific variant.
- `get_obj(i)` / `set_obj(i, val)` — object fields.
- `get_usize(i)` / `set_usize(i, val)` — `USize` fields.
- `get_num_{64,32,16,8}(i)` / `set_num_{64,32,16,8}(i, val)` — scalar fields.

Variant layouts are listed inside `[ … ]`, in tag order.

**Structure** — from `structure Point where x : Nat; y : Nat`:

```rust
lean_ffi::lean_inductive! { LeanPoint [ { num_obj: 2 } ] }

impl LeanPoint<LeanOwned> {
    pub fn mk(x: LeanNat<LeanOwned>, y: LeanNat<LeanOwned>) -> Self {
        let p = Self::alloc(0);
        p.set_obj(0, x);
        p.set_obj(1, y);
        p
    }
}
```

**Inductive** — from:

```lean
inductive CompareResult
  | matched
  | mismatch (leanSize rustSize : UInt64)
  | notFound
```

```rust
lean_ffi::lean_inductive! {
    LeanCompareResult [
        { },                // matched
        { num_64: 2 },      // mismatch
        { },                // notFound
    ]
}

// Build:
let m = LeanCompareResult::alloc(1);
m.set_num_64(0, lean_size);
m.set_num_64(1, rust_size);

// Read — dispatch on the Lean tag, then access fields:
match result.as_ctor().tag() {
    1 => {
        let (l, r) = (result.get_num_64(0), result.get_num_64(1));
    }
    _ => { /* matched | notFound */ }
}
```

For wrappers without a ctor layout (opaque externals, types represented as
`lean_box(n)`, etc.) use the lower-level `lean_domain_type!` macro.

[`LeanCtorLayout`]: https://docs.rs/lean-ffi/latest/lean_ffi/object/trait.LeanCtorLayout.html

### Constructor field layout

Lean
[reorders constructor fields](https://lean-lang.org/doc/reference/latest/The-Type-System/Inductive-Types/#run-time-inductives)
at runtime. Declaration order does **not** match memory order. For every
constructor, fields are laid out in this order:

1. Object fields (`lean_object*`) — declaration order.
2. `USize` fields — declaration order.
3. Fixed-size scalars — descending size (8B → 4B → 2B → 1B), then declaration
   order within each size.

So for

```lean
structure MyStruct where
  u8val  : UInt8
  obj    : Nat
  u32val : UInt32
  u64val : UInt64
```

the runtime order is `[obj, u64val, u32val, u8val]`. Trivial wrappers (e.g.
`Char` over `UInt32`) count as their underlying scalar.

Memory:

```
[header 8B] [object fields, 8B each] [USize fields, 8B each] [scalar bytes, descending size]
```

For `MyStruct` (`num_obj=1`, `num_usize=0`, `num_64=1`, `num_32=1`, `num_8=1`):

- `u64val` at scalar bytes 0–7
- `u32val` at scalar bytes 8–11
- `u8val` at scalar byte 12

`lean_inductive!` takes the per-size field counts and hands you size-indexed
accessors — `get_num_64(0)` for the first 8-byte scalar, `get_num_8(0)` for the
first 1-byte scalar, etc. No hand-rolled byte offsets:

```rust
lean_ffi::lean_inductive! {
    LeanMyStruct [ { num_obj: 1, num_64: 1, num_32: 1, num_8: 1 } ]
}

impl<R: LeanRef> LeanMyStruct<R> {
    pub fn obj(&self)    -> LeanBorrowed<'_> { self.get_obj(0) }
    pub fn u64val(&self) -> u64              { self.get_num_64(0) }
    pub fn u32val(&self) -> u32              { self.get_num_32(0) }
    pub fn u8val(&self)  -> u8               { self.get_num_8(0) }
}
```

For raw access (non-standard layouts, hand-tuned code), `LeanCtor` exposes
`get_u{8,16,32,64}(offset)` / `set_u{8,16,32,64}(offset, val)` with absolute
byte offsets matching `lean_ctor_get_uint*` / `lean_ctor_set_uint*`.

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

| Method              | C equivalent          | Description                                                        |
| ------------------- | --------------------- | ------------------------------------------------------------------ |
| `self.set(i, val)`  | `lean_array_set_core` | Set element (asserts exclusive — use for freshly allocated arrays) |
| `self.uset(i, val)` | `lean_array_uset`     | Set element (copies if shared)                                     |
| `self.push(val)`    | `lean_array_push`     | Append an element                                                  |
| `self.pop(self)`    | `lean_array_pop`      | Remove the last element                                            |
| `self.uswap(i, j)`  | `lean_array_uswap`    | Swap elements at `i` and `j`                                       |

#### `LeanByteArray`

| Method                | C equivalent                | Description                                                       |
| --------------------- | --------------------------- | ----------------------------------------------------------------- |
| `self.set_data(data)` | `lean_sarray_cptr` + memcpy | Bulk write (asserts exclusive — use for freshly allocated arrays) |
| `self.uset(i, val)`   | `lean_byte_array_uset`      | Set byte (copies if shared)                                       |
| `self.push(val)`      | `lean_byte_array_push`      | Append a byte                                                     |
| `self.copy()`         | `lean_copy_byte_array`      | Deep copy into a new exclusive array                              |

#### `LeanString`

| Method               | C equivalent         | Description                           |
| -------------------- | -------------------- | ------------------------------------- |
| `self.push(c)`       | `lean_string_push`   | Append a UTF-32 character             |
| `self.append(other)` | `lean_string_append` | Concatenate another string (borrowed) |

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
