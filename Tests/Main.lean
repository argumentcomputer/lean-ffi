import Tests.FFI
import Std.Data.HashMap

def main (args : List String) : IO UInt32 := do
  let suites : Std.HashMap String (List LSpec.TestSeq) := .ofList [
    ("borrowed",   Tests.FFI.borrowedSuite),
    ("owned",      Tests.FFI.ownedSuite),
    ("persistent", Tests.FFI.persistentSuite),
    ("property",   Tests.FFI.propertySuite),
  ]
  LSpec.lspecIO suites args
