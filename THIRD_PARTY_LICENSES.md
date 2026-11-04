# Third-party licenses

ReForge (Apache-2.0) depends on crates whose declared licenses are summarized in [`NOTICE`](NOTICE).

Most dependencies use permissive licenses (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib, MPL-2.0).
Before redistributing binaries, retain attribution required by those licenses.

Regenerate the inventory with:

```bash
cargo metadata --format-version 1 > /tmp/meta.json
# then rebuild NOTICE from package license fields
```
