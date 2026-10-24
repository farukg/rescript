let resolve = entities => switch entities {
| list{} | list{_} => Error("empty")
| list{first, second, ..._} => {
    Reference.resourceUrl(~base="https://example.invalid/workshop/", "glaze-chart.svg")
    ->Result.mapError(_ => "fixture-url")
    ->Result.flatMap(resource => Ok((first, second, resource)))
  }
}

let resolveWithoutBraces = entities => switch entities {
| list{} | list{_} => Error("empty")
| list{first, second, ..._} =>
  Reference.resourceUrl(~base="https://example.invalid/workshop/", "glaze-chart.svg")
  ->Result.mapError(_ => "fixture-url")
  ->Result.flatMap(resource => Ok((first, second, resource)))
}

type layout = Record | Array | Tuple | Block

let retainedLayouts = (layout, value) => switch layout {
| Record => {value: value}
| Array => [value]
| Tuple => (value, value)
| Block => {
    let next = transform(value)
    (next, next)
  }
}
