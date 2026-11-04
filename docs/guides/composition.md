# Composition guide

A composition manifest lists capability instances (`package_path`), optional event→command `bindings`, and `relationships` with `on_delete` policies (`unlink`, `reject`, `cascade`).

```json
{
  "id": "app.example",
  "capabilities": [
    {"instance": "notes", "package_path": "../../notes"},
    {"instance": "tasks", "package_path": "../../tasks"}
  ],
  "relationships": [{
    "id": "note-task-link",
    "from_instance": "notes",
    "to_instance": "tasks",
    "on_delete": "unlink"
  }]
}
```

Compile with `reforge compose --config composition.json`. Runtime link management uses the `reforge` instance action `reforge.link`.

Do not hardcode domain pairs into the engine; express policy in the manifest.
