import Tests.FFI
import Std.Data.HashMap

def main (args : List String) : IO UInt32 := do
  let suites : Std.HashMap String (List LSpec.TestSeq) := .ofList [
    ("ffi", Tests.FFI.suite),
  ]
  LSpec.lspecIO suites args
