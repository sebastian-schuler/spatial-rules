# Filter return shapes

Type: task
Status: needs-info

## Question

Which return-shape variant to ticket depends on the real filter endpoint's
response contract (from the post-v1 spec's "Open proposals" — deferred pending
a concrete consumer). The engine already returns the mask (`query`) and rich
per-candidate JSON (`queryRich`); callers hold the primitives, so these are
ergonomics, not capability.

Candidate additive napi methods:
- `filteredGeojson(candidates, query) -> String` — kept features as a GeoJSON
  string (pass-through `res.send`). **Default pick** if the endpoint returns
  the filtered FeatureCollection.
- `filteredFeatures(candidates, query) -> FeatureCollection` (JS objects) —
  for consumers that transform the data.
- `queryRich` object variant — JS array of objects instead of a string.
- `keep`-indices helper — kept feature indices for cheap slicing.

Needs: confirm what the internal filter endpoint returns (filtered
FeatureCollection vs transformed objects) — then ticket the matching variant,
default `filteredGeojson`.
