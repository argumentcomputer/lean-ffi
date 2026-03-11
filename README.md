# lean-ffi

Rust bindings to the `lean.h` Lean C FFI, generated with [`rust-bindgen`](https://github.com/rust-lang/rust-bindgen).
Bindgen runs in `build.rs` and generates unsafe Rust functions that link to 
Lean's `lean.h` C library. This external module can then be found at
`target/release/lean-ffi-<hash>/out/lean.rs`.

These bindings are then wrapped in a typed Rust API based on the underlying Lean type,
in order to make facilitate ergonomic handling of Lean objects in Rust.

## `LeanObject` API

The fundamental building block is `LeanObject`, a wrapper around an opaque
Lean value represented in Rust as `*const c_void`. This value is either a
pointer to a heap-allocated object or a tagged scalar (a raw value that fits
into one pointer's width, e.g. a `Bool` or small `Nat`). `LeanObject` is
then itself wrapped into Lean types such as `LeanCtor` for inductives,
`LeanArray` for arrays, etc.

A `lean_domain_type!` macro is also defined to allow for easy construction
of arbitrary Lean object types, which can then be used directly in FFI
functions to disambiguate between other `LeanObject`s. Some examples can be found in the
[Ix project](https://github.com/argumentcomputer/ix/blob/main/src/lean.rs).
To construct custom data in Rust, the user can define their own constructor methods
using `LeanCtor` (e.g. [`PutResponse`](https://github.com/argumentcomputer/ix/blob/main/src/ffi/iroh.rs)).
It is possible to use `LeanObject` or `*const c_void` directly in an `extern "C" fn`,
but this is generally not recommended as internal Rust functions may pass in the wrong object
more easily, and any low-level constructors would not be hidden behind the
API boundary. To enforce this, the `From<LeanType> for LeanObject` trait is
implemented to get the underlying `LeanObject`, but creating a wrapper type
from a `LeanObject` requires an explicit constructor for clarity.

A key concept in this design is that ownership of the data is transferred to
Lean, making it responsible for deallocation. If the data type is intended to be
used as a black box by Lean, `ExternalClass` is a useful abstraction. It
requires a function pointer for deallocation, meaning the Rust code must
provide a function that properly frees the object's memory by dropping it.
See [`KECCAK_CLASS`](https://github.com/argumentcomputer/ix/blob/main/src/ffi/keccak.rs) for an example.

## Notes

### Inductive Types

Extra care must be taken when dealing with [inductive
types](https://lean-lang.org/doc/reference/latest/The-Type-System/Inductive-Types/#run-time-inductives)
as the runtime memory layout of constructor fields may not match the
declaration order in Lean. Fields are reordered into three groups:

1. Non-scalar fields (lean_object *), in declaration order
2. `USize` fields, in declaration order
3. Other scalar fields, in decreasing order by size, then declaration order within each size

This means a structure like

```lean
structure Reorder where
  flag : Bool
  obj : Array Nat
  size : UInt64
```

would be laid out as [obj, size, flag] at runtime — the `UInt64` is placed
before the `Bool`. Trivial wrapper types (e.g. `Char` wraps `UInt32`) count as
their underlying scalar type.

To avoid issues, define Lean structures with fields already in runtime order
(objects first, then scalars in decreasing size), so that declaration order
matches the reordered layout.

### Enum FFI convention

Lean passes simple enums (inductives where all constructors have zero fields,
e.g. `DefKind`, `QuotKind`) as **raw unboxed tag values** (`0`, `1`, `2`, ...)
across the FFI boundary, not as `lean_box(tag)`. To decode, use
`obj.as_ptr() as usize`; to build, use `LeanObject::from_raw(tag as *const c_void)`.
Do **not** use `box_usize`/`unbox_usize` for these — doing so will silently
corrupt the value.

### Reference counting for reused objects

When building a new Lean object, if you construct all fields from scratch (e.g.
`LeanString::new(...)`, `LeanByteArray::from_bytes(...)`), ownership is
straightforward — the freshly allocated objects start with rc=1 and Lean manages
them from there.

However, if you take a Lean object received as a **borrowed** argument (`@&` in
Lean, `b_lean_obj_arg` in C) and store it directly into a new object via
`.set()`, you must call `.inc_ref()` on it first. Otherwise Lean will free the
original while the new object still references it. If you only read/decode the
argument into Rust types and then build fresh Lean objects, this does not apply.

### `lean_string_size` vs `lean_string_byte_size`

`lean_string_byte_size` returns the **total object memory size**
(`sizeof(lean_string_object) + m_size`), not the string data length.
Use `lean_string_size` instead, which returns `m_size` — the number of data
bytes including the NUL terminator. The `LeanString::byte_len()` wrapper handles
this correctly by returning `lean_string_size(obj) - 1`.

## License

MIT or Apache 2.0
