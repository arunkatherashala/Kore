# Maven Central v1.2.1 Fix Plan

## 🎯 Problem Summary
Maven Central deployment failing with `401 - Unauthorized` despite:
- ✅ Correct pom.xml with proper coordinates
- ✅ JAR building successfully
- ✅ Correct workflow structure

## 🔍 Root Cause
Maven authentication is not reaching the Nexus server. The `~/.m2/settings.xml` credentials are either:
1. Not being created with correct variable substitution
2. Not being read by Maven
3. Credentials in GitHub secrets are missing or incorrect

## ✅ Solution (Choose ONE)

### **Option 1: Verify GitHub Secrets (FASTEST - 5 min)**
Check if Maven secrets exist and are correct:

```bash
# Check if secrets exist
gh secret list -R arunkatherashala/Kore | grep MAVEN

# If missing, add them:
gh secret set MAVEN_USERNAME -R arunkatherashala/Kore
# Enter: [your Sonatype JIRA username]

gh secret set MAVEN_PASSWORD -R arunkatherashala/Kore
# Enter: [your Sonatype JIRA password OR token]
```

**How to get Sonatype credentials:**
1. Go to https://s01.oss.sonatype.org
2. Login with your Sonatype account
3. Go to Profile > User Token > View User Token
4. Copy the token (format: usually starts with a UUID-like string)
5. Use this token as MAVEN_PASSWORD

Then re-trigger workflow:
```bash
git tag -d v1.2.1
git tag v1.2.1
git push origin v1.2.1 -f
```

### **Option 2: Add GPG Signing (MORE SECURE - 15 min)**
Maven Central prefers signed JARs. Set up GPG:

1. Export your GPG private key:
```bash
gpg --export-secret-key --armor YOUR_KEY_ID > private.key
```

2. Add to GitHub secrets:
```bash
gh secret set MAVEN_GPG_PRIVATE_KEY -R arunkatherashala/Kore < private.key
gh secret set MAVEN_GPG_PASSPHRASE -R arunkatherashala/Kore
# Enter your GPG passphrase
```

3. Update `.github/workflows/publish-maven.yml` to import and use GPG key (template provided below)

4. Push and test:
```bash
git tag -d v1.2.1 && git tag v1.2.1 && git push origin v1.2.1 -f
```

### **Option 3: Manual Upload (WORKAROUND - 10 min)**
Until Maven auth is fixed, manually upload JAR:

1. Trigger Maven build locally or from workflow artifact:
```bash
cd maven
mvn clean package
# JAR is at: target/kore-fileformat-1.2.1.jar
```

2. Upload at https://central.sonatype.com/publishing/upload

## 🔧 Recommended Workflow Fix

Update `.github/workflows/publish-maven.yml` to use explicit Maven command:

```yaml
- name: Publish to Maven Central
  env:
    MAVEN_USERNAME: ${{ secrets.MAVEN_USERNAME }}
    MAVEN_PASSWORD: ${{ secrets.MAVEN_PASSWORD }}
  run: |
    if [ -d "maven" ]; then
      cd maven
      mvn clean deploy \
        -Dusername="$MAVEN_USERNAME" \
        -Dpassword="$MAVEN_PASSWORD" \
        -DskipTests
    fi
  continue-on-error: true
```

This approach passes credentials directly via Maven properties instead of relying on settings.xml.

## 📋 Verification Checklist

- [ ] GitHub secrets `MAVEN_USERNAME` and `MAVEN_PASSWORD` are set
- [ ] Secrets contain valid Sonatype credentials
- [ ] Workflow file uses correct syntax for variable passing
- [ ] Latest workflow changes are committed to main
- [ ] Tag v1.2.1 is recreated with updated commits
- [ ] Maven workflow runs and completes
- [ ] Check Maven Central: https://search.maven.org/?q=kore-fileformat&central=true

## ⏱️ Timeline

| Step | Time | Status |
|------|------|--------|
| Verify secrets | 5 min | **START HERE** |
| Test Maven workflow | 2 min | Automated |
| Verify on Maven Central | 2-5 min | Allow time for indexing |
| **Total** | **10-15 min** | 🚀 **Ready!** |

## 🎯 Next Steps

1. **RIGHT NOW:** Check if MAVEN_USERNAME and MAVEN_PASSWORD secrets are set
   ```bash
   gh secret list -R arunkatherashala/Kore
   ```

2. **If missing:** Add the secrets using your Sonatype credentials

3. **Recreate tag and push:**
   ```bash
   git tag -d v1.2.1
   git tag v1.2.1
   git push origin v1.2.1 -f
   ```

4. **Monitor workflow:**
   ```bash
   gh run list --workflow="publish-maven.yml" -R arunkatherashala/Kore --limit 1
   ```

5. **Verify on Maven Central (wait 1-5 min for indexing):**
   ```bash
   curl "https://search.maven.org/solrsearch/select?q=g:io.github.arunkatherashala+AND+a:kore-fileformat&rows=10&wt=json"
   ```

## 🆘 If Still Not Working

Check workflow logs:
```bash
gh run view $(gh run list --workflow="publish-maven.yml" -R arunkatherashala/Kore --limit 1 --json databaseId -q) -R arunkatherashala/Kore --log | grep -i "401\|403\|error\|unauthorized"
```

Common issues:
- **401 Unauthorized** → Credentials wrong or empty
- **403 Forbidden** → Account doesn't have publishing rights
- **Nexus timeout** → Network issue, retry later

---

**Status:** Maven needs 1 quick check (secrets verification) to fix ✅  
**Estimated Fix Time:** 10-15 minutes  
**Current Platforms:** 6/8 live ✨

Update this document after testing!
