# 🐍 DEPLOY TO PYPI - INTERACTIVE GUIDE

## STEP 1: Get Your PyPI Token

Go to: https://pypi.org/manage/account/tokens/

1. Log in to PyPI
2. Click "Add API token"
3. Name it: "kore-fileformat-deploy"
4. Copy the token (starts with `pypi-`)

**Keep this token secret!** You'll use it only once.

---

## STEP 2: Upload to PyPI

Run this command in the Kore directory:

```bash
twine upload dist/*
```

When prompted for username, enter: `__token__`
When prompted for password, paste your token: `pypi-AgE...`

---

## That's it! 🚀

Wait ~5 minutes and check: https://pypi.org/project/kore-fileformat/1.3.3/

Your package will be live!
