// Consumer under src/b that depends on a genType peer in src/a.
// genType must emit `from '../a/CrossDirPeer.gen'`, not `./CrossDirPeer.gen`
// and not `../../src/a/CrossDirPeer.gen`.

@genType
type bundle = {
  peer: CrossDirPeer.peer,
  note: string,
}

@genType
let wrap = (peer: CrossDirPeer.peer, note: string): bundle => {
  peer,
  note,
}
