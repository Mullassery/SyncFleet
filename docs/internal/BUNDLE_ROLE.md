# Bundle role — Robotics data & fleet loop (deferred)

Portfolio-level decision: **deferred**. See
[`Mullassery/BUNDLES.md`](../Mullassery/blob/main/BUNDLES.md).

- **Role:** Peer-to-peer fleet state replication
- **Upstream/downstream:** PyTerrainMap, PyRoboReplay, fleet deployments
- **Unlock condition:** Conflict resolution runtime, schema evolution, PyPI wrapper, then a 3-node demo.
- **Current status:** standalone; not a dependency of the other bundle members.

Do not add cross-repo runtime dependencies until the unlock condition is met.
When it is, add a versioned interface (not a git URL), a shared
observation/mission schema, and an end-to-end demo.
