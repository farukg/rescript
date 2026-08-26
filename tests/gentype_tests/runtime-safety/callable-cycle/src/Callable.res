@module("./CyclePeer.mjs")
external touch: unit => unit = "touch"

@genType
let increment = (value: int): int => value + 1

touch()
