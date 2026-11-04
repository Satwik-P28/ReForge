# Export guide

```bash
reforge export \
  --config examples/compositions/notes-tasks/composition.json \
  --out /tmp/notes-tasks-app \
  --workspace . \
  --target local-web
```

The export directory contains `application.ir.json`, `packages/`, `vendor/`, `composition.reexport.json`, and `app/`. Build with Cargo from that directory only. Persistence uses `./data/app.sqlite`.

Acceptance: `cargo test -p reforge --test e2e_export`.
