# Stashi Wallet guide tooling

The PDF is generated from the Markdown and screenshots under `docs/user-guide/`.
Do not edit the PDF directly.

## Build locally

Use Python 3.12 or newer:

```bash
python -m pip install --no-deps --requirement scripts/docs/requirements.txt
python scripts/docs/build_stashi_user_guide.py
python scripts/docs/verify_stashi_user_guide.py
```

The default output is `output/pdf/Stashi-Wallet-User-Guide.pdf`. Use
`--output <path>` when building a review copy elsewhere.

## Editorial and accessibility requirements

The published guide uses British English and the term `Mobile` for handheld
layouts. Markdown image descriptions become alternative text in the generated
PDF. The generator also creates document structure tags, a single level-one
heading for the product name, level-two chapter headings, tagged lists and
tables, bookmarks, and a British English document language declaration.

The verifier rejects missing image descriptions, disallowed editorial terms,
an invalid heading hierarchy, untagged pages, or missing accessibility
metadata. Run it after every guide build.

## Publication

`.github/workflows/user-guide-pages.yml` builds and validates the guide when its
source, screenshots, logo, or tooling changes.

- Pull requests receive a downloadable review artifact and never deploy.
- Changes on `main` publish the approved guide to GitHub Pages.
- A manual run from `main` rebuilds and republishes the guide when needed.

The workflow deploys a generated Pages artifact. It does not create bot commits
or write generated files back to the repository.
