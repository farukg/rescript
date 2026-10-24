import {add} from "./src/AliasConsumer.gen.ts"

if (add(20, 22) !== 42) {
  throw new Error("module-alias type projection changed runtime behavior")
}
