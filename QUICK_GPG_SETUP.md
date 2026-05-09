# Maven Central GPG Signing - Quick Setup Guide

## Quick Manual Setup (2-3 minutes)

Since GPG installation on Windows can be tricky, here's the fastest manual path:

### Step 1: Open Git Bash and Generate Key

Click on your Kore folder in Windows Explorer:
- Right-click → "Open Git Bash Here"

Then paste and run this ONE command:

```bash
gpg --quick-gen-key "Arun Kather Ashala <arunkatherashala@gmail.com>" rsa4096 never
```

Wait a moment, then continue.

### Step 2: Export Your Private Key

In the same Git Bash window, run:

```bash
gpg --list-secret-keys --keyid-format SHORT
```

Look for the output like:
```
sec   rsa4096/ABCD1234
```

Copy the key ID (the 8-char code after the `/`), then export it:

```bash
# Replace ABCD1234 with your actual key ID
gpg --export-secret-keys --armor ABCD1234
```

This will print your PRIVATE KEY to the screen. Select all the output (Ctrl+A), copy it (Ctrl+C).

### Step 3: Store as GitHub Secret

1. Go to: https://github.com/arunkatherashala/Kore/settings/secrets/actions
2. Click **"New repository secret"**
3. Name: `GPG_PRIVATE_KEY`
4. Value: Paste what you copied (Ctrl+V)
5. Click **"Add secret"**

6. Click **"New repository secret"** again
7. Name: `GPG_PASSPHRASE`
8. Value: Leave **empty** (no passphrase)
9. Click **"Add secret"**

### Step 4: Deploy!

In PowerShell at your Kore folder, run:

```powershell
gh workflow run publish-maven.yml --ref release/v0.1.0
```

Done! The deployment will start automatically.

---

## Troubleshooting

**"gpg not found"?**
- Use Git Bash (right-click folder → "Open Git Bash Here")
- Git includes GPG automatically

**"Key generation failed"?**
- Close and re-open Git Bash
- Try the command again

**"Can't find key ID"?**
- Make sure you ran the --quick-gen-key command first
- Wait 5 seconds between commands

---

## What's Happening

The workflow will:
1. Import your GPG key from the secret
2. Build the JAR files
3. Sign them with your GPG key
4. Upload to Maven Central
5. Maven Central validates the signatures
6. Package is published!

All 6 platforms will be LIVE! 🚀
