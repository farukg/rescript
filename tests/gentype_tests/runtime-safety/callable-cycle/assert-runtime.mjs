import {increment} from "./src/Callable.gen.ts"
import {observed} from "./src/CyclePeer.mjs"

if (increment(1) !== 2 || observed !== 42) {
  throw new Error("TDZ-safe callable export did not preserve runtime behavior")
}
