#!/bin/bash
# Generate a GPG key locally for Maven Central signing
# Run this on your local machine, NOT in CI

cat > /tmp/gen_key.batch <<EOF
%echo Generating a GPG key for Maven Central
Key-Type: RSA
Key-Length: 4096
Subkey-Type: RSA
Subkey-Length: 4096
Name-Real: Arun Kather Ashala
Name-Email: arunkatherashala@gmail.com
Expire-Date: 0
%no-protection
%commit
%echo done
EOF

echo "Generating GPG key..."
gpg --batch --generate-key /tmp/gen_key.batch

echo ""
echo "Key generated! Now list your keys:"
gpg --list-secret-keys --keyid-format LONG

echo ""
echo "To export your private key for GitHub Actions, run:"
echo "  gpg --export-secret-keys --armor <KEY_ID> > private.key"
echo ""
echo "Then:"
echo "1. Copy the contents of private.key"
echo "2. Add it as a GitHub Secret named 'GPG_PRIVATE_KEY'"
echo "3. Add your GPG passphrase as a GitHub Secret named 'GPG_PASSPHRASE'"
