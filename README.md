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
(`sizeof(lean_string_object) + m_size`), not the string data length.
Use `lean_string_size` instead, which returns `m_size` — the number of data
bytes including the NUL terminator. The `LeanString::byte_len()` wrapper handles
this correctly by returning `lean_string_size(obj) - 1`.

## License

MIT or Apache 2.0
