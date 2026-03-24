# lean-ffi

Rust bindings to the `lean.h` Lean C FFI, generated with [`rust-bindgen`](https://github.com/rust-lang/rust-bindgen).
Bindgen runs in `build.rs` and generates unsafe Rust functions that link to
Lean's `lean.h` C library. This external module can then be found at
`target/release/lean-ffi-<hash>/out/lean.rs`.

These bindings are then wrapped in a typed Rust API that models Lean's
ownership conventions (`lean_obj_arg` vs `b_lean_obj_arg`) using Rust's
type system.

## Ownership Model

The core types are:

- **`LeanOwned`** — An owned reference to a Lean object. `Drop` calls `lean_dec`,
  `Clone` calls `lean_inc`. Not `Copy`. Corresponds to `lean_obj_arg` (input) and
  `lean_obj_res` (output) in the C FFI.

- **`LeanBorrowed<'a>`** — A borrowed reference. `Copy`, no `Drop`, lifetime-bounded.
  Corresponds to `b_lean_obj_arg` in the C FFI. Used when Lean declares a parameter
  with `@&`.

- **`LeanShared`** — A thread-safe owned reference. Wraps `LeanOwned` after calling
  `lean_mark_mt` on the object graph, which transitions all reachable objects to
  multi-threaded mode with atomic refcounting. `Send + Sync`. Use `borrow()` to get
  a `LeanBorrowed<'_>` for reading, `into_owned()` to unwrap back to `LeanOwned`.

- **`LeanRef`** — Trait implemented by `LeanOwned`, `LeanBorrowed`, and `LeanShared`,
  providing shared read-only operations like `as_raw()`, `is_scalar()`, `tag()`, and
  unboxing methods.

All reference types are safe for persistent objects (`m_rc == 0`) — `lean_inc_ref` and
`lean_dec_ref` are no-ops when `m_rc == 0`.

## Domain Types

Domain types wrap the ownership types to provide type safety at FFI boundaries.
Built-in domain types include `LeanArray<R>`, `LeanString<R>`, `LeanCtor<R>`,
`LeanList<R>`, `LeanOption<R>`, `LeanExcept<R>`, `LeanIOResult<R>`, `LeanProd<R>`,
`LeanNat<R>`, `LeanBool<R>`, `LeanByteArray<R>`, and `LeanExternal<T, R>`.

### Naming convention

Domain types are prefixed with `Lean` to distinguish them from Lean-side type names
and to match the built-in types. For example, a Lean `Point` structure becomes
`LeanPoint` in Rust.

### Defining custom domain types

Use the `lean_domain_type!` macro to define newtypes for your Lean types:

```rust
lean_ffi::lean_domain_type! {
    /// Lean `Point` — structure Point where x : Nat; y : Nat
    LeanPoint;
    /// Lean `PutResponse` — structure PutResponse where message : String; hash : String
    LeanPutResponse;
}
```

This generates a `#[repr(transparent)]` wrapper with `Clone`, conditional `Copy`,
`inner()`, `as_raw()`, `into_raw()`, and `From` impls. You can then add
accessor methods — readers are generic over `R: LeanRef` (work on both owned
and borrowed), constructors return `LeanOwned`:

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

### External objects (`LeanExternal<T, R>`)

External objects let you store arbitrary Rust data inside a Lean object. Lean
sees an opaque type; Rust controls allocation, access, mutation, and cleanup.

**Register** an external class exactly once, using `OnceLock` or `LazyLock`:

```rust
use std::sync::LazyLock;
use lean_ffi::object::{ExternalClass, LeanExternal, LeanOwned, LeanBorrowed};

struct Hasher { state: Vec<u8> }

// register_with_drop<T> generates a finalizer that calls drop(Box::from_raw(ptr))
// and a no-op foreach (no Lean objects inside T to traverse).
static HASHER_CLASS: LazyLock<ExternalClass> =
    LazyLock::new(ExternalClass::register_with_drop::<Hasher>);
```

**Create** — `LeanExternal::alloc` boxes the value and returns an owned
external object:

```rust
// Lean: @[extern "rs_hasher_new"] opaque Hasher.new : Unit → Hasher
#[unsafe(no_mangle)]
extern "C" fn rs_hasher_new(_unit: LeanOwned) -> LeanExternal<Hasher, LeanOwned> {
    LeanExternal::alloc(&HASHER_CLASS, Hasher { state: Vec::new() })
}
```

**Read** — `.get()` borrows the stored `&T`. Works on both owned and borrowed
references:

```rust
// Lean: @[extern "rs_hasher_bytes"] opaque Hasher.bytes : @& Hasher → ByteArray
#[unsafe(no_mangle)]
extern "C" fn rs_hasher_bytes(
    h: LeanExternal<Hasher, LeanBorrowed<'_>>,  // @& → borrowed
) -> LeanByteArray<LeanOwned> {
    LeanByteArray::from_bytes(&h.get().state)  // &Hasher — no clone, no refcount change
}
```

**Update** — `.get_mut()` returns `Option<&mut T>`, which is `Some` when the
object is exclusively owned (`m_rc == 1`). This enables
in-place mutation without allocating a new external object. When shared `.get_mut()`
returns `None` and instead clones into a new object on write.

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
- `LeanExternal<T, LeanBorrowed<'_>>` — no refcount changes, no cleanup.
  Use for `@&` parameters.
- Converting to `LeanOwned` (e.g. to store in a ctor field): call `.into()`.


### FFI function signatures

Use domain types in `extern "C"` function signatures. The ownership type parameter
tells Rust how to handle reference counting:

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

## Inductive Types and Field Layout

Extra care must be taken when dealing with [inductive
types](https://lean-lang.org/doc/reference/latest/The-Type-System/Inductive-Types/#run-time-inductives)
as the runtime memory layout of constructor fields may not match the
declaration order in Lean. Fields are reordered into three groups:

1. Non-scalar fields (`lean_object*`), in declaration order
2. `USize` fields, in declaration order
3. Other scalar fields, in decreasing order by size, then declaration order within each size

This means a structure like

```lean
structure Reorder where
  flag : Bool
  obj : Array Nat
  size : UInt64
```

would be laid out as `[obj, size, flag]` at runtime — the `UInt64` is placed
before the `Bool`. Trivial wrapper types (e.g. `Char` wraps `UInt32`) count as
their underlying scalar type.

Use `LeanCtor` methods to access fields at the correct offsets:

```rust
// 1 object field, scalars: u64 at offset 0, u8 (Bool) at offset 8
let ctor = unsafe { LeanBorrowed::from_raw(ptr.as_raw()) }.as_ctor();
let obj = ctor.get(0);              // object field by index
let size = ctor.get_u64(1, 0);      // u64 at scalar offset 0 (past 1 non-scalar field)
let flag = ctor.get_bool(1, 8);     // bool at scalar offset 8
```

## In-Place Mutation

Lean's runtime supports in-place mutation when an object is **exclusively owned**
(`m_rc == 1`, single-threaded mode). When shared, the object is copied first.
`LeanRef::is_exclusive()` exposes this check.

These methods consume `self` and return a (possibly new) object, mutating in
place when exclusive or copying first when shared:

### `LeanArray`

| Method | C equivalent | Description |
|--------|--------------|-------------|
| `set(&self, i, val)` | `lean_array_set_core` | Set element (asserts exclusive — use for freshly allocated arrays) |
| `uset(self, i, val)` | `lean_array_uset` | Set element (copies if shared) |
| `push(self, val)` | `lean_array_push` | Append an element |
| `pop(self)` | `lean_array_pop` | Remove the last element |
| `uswap(self, i, j)` | `lean_array_uswap` | Swap elements at `i` and `j` |

### `LeanByteArray`

| Method | C equivalent | Description |
|--------|--------------|-------------|
| `set_data(&self, data)` | `lean_sarray_cptr` + memcpy | Bulk write (asserts exclusive — use for freshly allocated arrays) |
| `uset(self, i, val)` | `lean_byte_array_uset` | Set byte (copies if shared) |
| `push(self, val)` | `lean_byte_array_push` | Append a byte |
| `copy(self)` | `lean_copy_byte_array` | Deep copy into a new exclusive array |

### `LeanString`

| Method | C equivalent | Description |
|--------|--------------|-------------|
| `push(self, c)` | `lean_string_push` | Append a UTF-32 character |
| `append(self, other)` | `lean_string_append` | Concatenate another string (borrowed) |

`LeanExternal<T>` also supports in-place mutation via `get_mut()` — see the
**Update** section under [External objects](#external-objects-leanexternalt-r).

## Notes

### Rust panic behavior

By default, Rust uses stack unwinding for panics. If a panic occurs in a Lean-to-Rust FFI function, the unwinding will try to cross the FFI boundary back into Lean. This is [undefined behavior](https://doc.rust-lang.org/stable/reference/panic.html#unwinding-across-ffi-boundaries). To avoid this, configure Rust to abort on panic in `Cargo.toml`:

```toml
[profile.release]
panic = "abort"
```

### Enum FFI convention

Lean passes simple enums (inductives where all constructors have zero fields,
e.g. `DefKind`, `QuotKind`) as **raw unboxed tag values** (`0`, `1`, `2`, ...)
across the FFI boundary, not as `lean_box(tag)`. Use `LeanOwned::from_enum_tag()`
and `LeanRef::as_enum_tag()` for these.

### Persistent objects

Module-level Lean definitions and objects in compact regions are persistent
(`m_rc == 0`). Both `lean_inc_ref` and `lean_dec_ref` are no-ops for persistent
objects, so `LeanOwned`, `LeanBorrowed`, `Clone`, and `Drop` all work correctly
without special handling.

### `lean_string_size` vs `lean_string_byte_size`

`lean_string_byte_size` returns the **total object memory size**
(`sizeof(lean_string_object) + capacity`), not the string data length.
Use `lean_string_size` instead, which returns `m_size` — the number of data
bytes including the NUL terminator. `LeanString` wraps these correctly:

- `byte_len()` — data bytes excluding NUL (`m_size - 1`)
- `length()` — UTF-8 character count (`m_length`)
- `as_str()` — view as `&str`

## License

MIT or Apache 2.0
