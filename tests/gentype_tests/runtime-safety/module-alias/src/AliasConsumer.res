module RuntimeAlias = Owner

@genType
type exposed = RuntimeAlias.t

@genType
let add = (left: exposed, right: exposed): exposed => RuntimeAlias.add(left, right)
