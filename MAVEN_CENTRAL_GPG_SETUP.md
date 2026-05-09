# Maven Central GPG Signing Setup

This guide explains how to set up GPG signing for Maven Central deployments.

## Step 1: Generate a GPG Key Locally

On your local Windows machine, open PowerShell and run:

```powershell
# Install GPG if you don't have it (via Chocolatey or from https://gpg4win.org/)
choco install gpg

# Or if you already have gpg, verify it works:
gpg --version
```

Then generate the key:

```bash
gpg --full-generate-key
```

When prompted:
- **Key type:** Choose RSA and RSA (default, option 1)
- **Key length:** Enter 4096
- **Validity:** Enter 0 (no expiration)
- **Real name:** Arun Kather Ashala
- **Email:** arunkatherashala@gmail.com
- **Passphrase:** Create a strong passphrase and **REMEMBER IT**

## Step 2: Export Your Private Key

List your keys to find your key ID:
```bash
gpg --list-secret-keys --keyid-format LONG
```

Look for something like:
```
sec   rsa4096/ABCD1234EFGH5678 2026-05-09
```

The key ID is `ABCD1234EFGH5678` (the part after the `/`).

Export your private key:
```bash
gpg --export-secret-keys --armor ABCD1234EFGH5678 > private-key.asc
```

This creates a file `private-key.asc` with your private key.

## Step 3: Add Secrets to GitHub

1. Go to: https://github.com/arunkatherashala/Kore/settings/secrets/actions

2. **Click "New repository secret"** and add:
   - **Name:** `GPG_PRIVATE_KEY`
   - **Value:** Paste the entire contents of `private-key.asc` file

3. **Click "New repository secret"** again:
   - **Name:** `GPG_PASSPHRASE`
   - **Value:** The passphrase you created in Step 1

4. **Click "Save"**

## Step 4: Done!

Your GitHub Actions workflow will now automatically import your GPG key and use it to sign artifacts when deploying to Maven Central.

## Security Notes

- ⚠️ **IMPORTANT:** Your private key is now encrypted in GitHub Secrets. Only you can see/modify it.
- The passphrase is masked in action logs.
- If you ever compromise your key, you can regenerate it and update the secrets.
- Never share your private key or passphrase with anyone.

## Troubleshooting

If the deployment still fails, check:
1. GPG_PRIVATE_KEY secret contains the full key (with `-----BEGIN PGP PRIVATE KEY BLOCK-----` at the start)
2. GPG_PASSPHRASE is correct (the passphrase you entered when creating the key)
3. The namespace `io.github.arunkatherashala` is registered in Maven Central Portal
