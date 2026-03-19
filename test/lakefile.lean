import Lake
open System Lake DSL

package «lean-ffi-test» where
  version := v!"0.1.0"

require LSpec from git
  "https://github.com/argumentcomputer/LSpec" @ "928f27c7de8318455ba0be7461dbdf7096f4075a"

lean_lib Tests

@[test_driver]
lean_exe LeanFFITests where
  root := `Tests.Main

section FFI

/-- Build the static lib for the Rust test crate -/
extern_lib lean_ffi_rs pkg := do
  proc { cmd := "cargo", args := #["build", "--release"], cwd := pkg.dir } (quiet := true)
  let libName := nameToStaticLib "lean_ffi_rs"
  inputBinFile $ pkg.dir / ".." / "target" / "release" / libName

end FFI

