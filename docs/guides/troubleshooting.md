# Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `compose.unsafe_path` | Null bytes in package_path | Use relative paths; compiler canonicalizes `..` |
| Package not found | Bad relative path from composition file | Paths are relative to the composition file directory |
| Export build fails | Missing vendored crate rewrite | Re-run export from a clean out dir; ensure `--workspace` points at ReForge root |
| Studio bind refused | Non-loopback address | Use `127.0.0.1` |
| Event loop error | Cyclic bindings | Remove cycles or reduce propagation; max depth is 32 |
| Delete rejected | `on_delete: reject` with live links | Unlink first or change policy |
