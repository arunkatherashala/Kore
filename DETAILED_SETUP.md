# Detailed Setup: Docker Hub & Maven Central

## DOCKER HUB - COMPLETE WALKTHROUGH

### Step 1: Sign Up (2 minutes)
```
1. Open: https://hub.docker.com/signup
2. Fill form:
   - Email: [your email]
   - Username: [choose something like "yourusername"]
   - Password: [strong password]
3. Click "Sign up"
4. Check your email for verification link
5. Click the link in the email to verify
6. You're now on Docker Hub dashboard!
```

### Step 2: Create Personal Access Token (3 minutes)
```
1. Look at TOP RIGHT corner → Click on your PROFILE ICON
2. From dropdown, click "Account settings"
3. In LEFT MENU, click "Security"
4. You'll see a section: "Personal access tokens"
5. Click BLUE BUTTON: "New access token"
6. In the form:
   - Token name: "github-actions-kore"
   - Description: "For automating GitHub releases"
   - Access permissions: Check "Read & Write"
   - Click "Generate"
7. A token will appear (looks like: dckr_pat_abc123xyz...)
8. COPY THIS TOKEN and save it somewhere safe!
```

### What You'll Have:
- **DOCKER_USERNAME** = the username you created in Step 1
- **DOCKER_PASSWORD** = the token from Step 2

---

## MAVEN CENTRAL (OSSRH) - COMPLETE WALKTHROUGH

### Step 1: Create OSSRH Account (3 minutes)
```
1. Open: https://issues.sonatype.org/secure/Signup!default.jspa
2. Fill the form:
   - Full Name: [Your Name]
   - Email: [your email]
   - Username: [your username - like "yourusername"]
   - Password: [strong password]
   - CAPTCHA: Check "I'm not a robot"
3. Click "Sign up"
4. Check your email for confirmation
5. Click confirmation link
6. You now have OSSRH account!
```

### What You'll Have:
- **OSSRH_USERNAME** = username from Step 1
- **OSSRH_PASSWORD** = password from Step 1

---

## GPG KEY (Optional - for signing)

If you want to sign your Maven packages:

### On Windows:
```powershell
# Install GnuPG
choco install gnupg -y

# Generate key
gpg --full-generate-key

# When asked:
# - Key type: RSA (default)
# - Key size: 4096
# - Expiration: 0 (no expiration)
# - Real name: Your Name
# - Email: your@email.com
# - Passphrase: Create a strong one (this is MAVEN_GPG_PASSPHRASE)

# List keys (to verify)
gpg --list-keys
```

### On Mac:
```bash
# Install GnuPG
brew install gnupg

# Generate key
gpg --full-generate-key
# Same questions as above
```

### Upload Key to Keyserver:
```bash
gpg --list-keys
# Copy your key ID (8-character hex)
gpg --send-keys YOUR_KEY_ID --keyserver hkp://keyserver.ubuntu.com
```

---

## ADDING CREDENTIALS TO GITHUB

Once you have all your credentials, add them here:

### 1. Open GitHub Repository Settings
```
1. Go to: https://github.com/arunkatherashala/Kore
2. Click Settings (top menu)
3. Click "Secrets and variables" (left menu)
4. Click "Actions"
```

### 2. Add Docker Secrets
```
Click "New repository secret"

First Secret:
- Name: DOCKER_USERNAME
- Value: [paste your Docker username]
- Click "Add secret"

Second Secret:
- Name: DOCKER_PASSWORD
- Value: [paste your Docker access token]
- Click "Add secret"
```

### 3. Add Maven Secrets
```
Click "New repository secret"

First Secret:
- Name: OSSRH_USERNAME
- Value: [paste your OSSRH username]
- Click "Add secret"

Second Secret:
- Name: OSSRH_PASSWORD
- Value: [paste your OSSRH password]
- Click "Add secret"

Optional - If you created GPG key:
- Name: MAVEN_GPG_PASSPHRASE
- Value: [paste your GPG passphrase]
- Click "Add secret"
```

---

## VERIFY SECRETS WERE ADDED

```bash
# Check they're there (they won't show the values)
gh secret list
```

---

## TRIGGER DEPLOYMENTS

Once secrets are added:

```bash
# Deploy to Docker Hub
gh workflow run publish-docker.yml --ref v0.1.0

# Deploy to Maven Central
gh workflow run publish-maven.yml --ref v0.1.0

# Monitor the workflows
gh run list
gh run view <run-id> --log
```

---

## SUMMARY OF WHAT YOU NEED

| Credential | Where From | Used For |
|-----------|-----------|----------|
| DOCKER_USERNAME | Docker Hub account | Pushing to Docker registry |
| DOCKER_PASSWORD | Docker Hub > Security > New Token | Authenticating to Docker |
| OSSRH_USERNAME | Sonatype JIRA account | Maven Central authentication |
| OSSRH_PASSWORD | Sonatype JIRA password | Maven Central authentication |
| MAVEN_GPG_PASSPHRASE | Your GPG key (optional) | Signing packages |

---

## STILL CONFUSED?

1. **For Docker Hub Help**: https://docs.docker.com/docker-hub/access-tokens/
2. **For Maven Central Help**: https://central.sonatype.org/publishing/publish-maven/
3. **For GPG Help**: https://docs.github.com/en/authentication/managing-commit-signature-verification/generating-a-new-gpg-key

You can do this! Just follow one step at a time. 🚀
