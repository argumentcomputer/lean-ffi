import Lake
open System Lake DSL

package «lean-ffi-test» where
  version := v!"0.1.0"

require LSpec from git
  "https://github.com/argumentcomputer/LSpec" @ "928f27c7de8318455ba0be7461dbdf7096f4075a"

section FFI

/-- Build the static lib for the Rust FFI test crate -/
extern_lib ffi_rs_test pkg := do
  proc { cmd := "cargo", args := #["build", "--release", "--features", "test-ffi"], cwd := pkg.dir } (quiet := true)
  let srcName := nameToStaticLib "lean_ffi"
  let dstName := nameToStaticLib "lean_ffi_test"
  let releaseDir := pkg.dir / "target" / "release"
  proc { cmd := "cp", args := #["-f", srcName, dstName], cwd := releaseDir }
  inputBinFile $ releaseDir / dstName

end FFI

lean_lib Tests

@[test_driver]
lean_exe LeanFFITests where
  root := `Tests.Main
