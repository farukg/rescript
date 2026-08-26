// Peer type living under src/a — imported across subdirs by CrossDirConsumer.

@genType
type peer = {
  id: string,
  label: string,
}

@genType
let make = (~id: string, ~label: string): peer => {id, label}
