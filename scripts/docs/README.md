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

## Publication

`.github/workflows/user-guide-pages.yml` builds and validates the guide when its
source, screenshots, logo, or tooling changes.

- Pull requests receive a downloadable review artifact and never deploy.
- Changes on `main` publish the approved guide to GitHub Pages.
- A manual run from `main` rebuilds and republishes the guide when needed.

The workflow deploys a generated Pages artifact. It does not create bot commits
or write generated files back to the repository.
