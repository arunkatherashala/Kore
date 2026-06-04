# Maven Central Deployment - Sonatype Configuration Issues

## Current Status: 402 Payment Required

### Issue
Maven deployment is failing with HTTP 402 (Payment Required) error when attempting to upload to OSSRH.

**Error Message:**
```
Failed to deploy artifacts: Could not transfer artifact 
io.github.arunkatherashala:kore-fileformat:pom:1.3.3 from/to ossrh
(https://oss.sonatype.org/service/local/staging/deploy/maven2/): 
status code: 402, reason phrase: Payment Required (402)
```

### What This Means
HTTP 402 typically indicates one of:
1. **Namespace not registered** - The groupId `io.github.arunkatherashala` may not be registered with Sonatype
2. **Account restriction** - The Sonatype account `je76g3` may need additional setup
3. **Credentials invalid** - The username/password in settings.xml may be incorrect for this deployment

---

## Configuration Summary

### Maven Installation ✅
- **Status:** Successfully installed
- **Version:** Apache Maven 3.9.6
- **Location:** C:\Maven\apache-maven-3.9.6
- **Java:** OpenJDK 17.0.2
- **PATH:** Configured

### Credentials Configuration ✅
- **Username:** je76g3
- **Settings File:** C:\Users\ksak_\.m2\settings.xml
- **Format:** Proper OSSRH server configuration

### pom.xml Configuration ✅
- **GroupId:** io.github.arunkatherashala
- **ArtifactId:** kore-fileformat
- **Version:** 1.3.3
- **Repository:** OSSRH Legacy Endpoint (oss.sonatype.org)

---

## Recommended Solutions (In Order)

### Option 1: Verify Sonatype Account Setup
1. Go to: https://identity.sonatype.com/
2. Log in with credentials: `je76g3` / `iAyH2RS1ZK3PFBbOksv6oRRe37jwVRQNH`
3. Check:
   - Account status (Active/Pending/Suspended)
   - Namespace approval for `io.github.arunkatherashala`
   - Any billing/subscription requirements

### Option 2: Verify GroupId Registration
1. Check if `io.github.arunkatherashala` is registered:
   - https://issues.sonatype.org/ (JIRA login required)
   - Search for existing namespace tickets
   - If not registered, create new JIRA issue for namespace approval

### Option 3: Use Alternative GroupId
If namespace is not approved, alternative options:
```xml
<!-- Option A: GitHub-based namespace -->
<groupId>com.github.arunkatherashala</groupId>

<!-- Option B: Personal domain -->
<groupId>com.katherashala</groupId>

<!-- Option C: Short namespace -->
<groupId>io.kore</groupId>
```

### Option 4: Direct Upload to Maven Central
For accounts with direct upload permissions:
1. Visit: https://central.sonatype.com/
2. Go to "Uploads" section
3. Use web interface to upload artifacts directly

### Option 5: Contact Sonatype Support
1. Email: support@sonatype.com
2. Provide:
   - Username: je76g3
   - Project: kore-fileformat v1.3.3
   - GroupId: io.github.arunkatherashala
   - Error code: 402

---

## Quick Test Commands

### Test Maven Connectivity
```powershell
# Verify Maven settings
mvn help:describe -Dplugin=deploy -Dfull

# Dry-run deploy (no actual upload)
mvn deploy -DaltDeploymentRepository=local::default::file://./target/repo
```

### Check OSSRH Credentials
```powershell
# Test with verbose output
mvn clean deploy -DskipTests -X 2>&1 | Select-String "Authentication|401|402|403"
```

### Alternative: Use New Sonatype Central
For newer Sonatype Central accounts:
```xml
<repository>
  <id>central</id>
  <url>https://central.sonatype.com/api/v1/publisher/upload</url>
</repository>
```

---

## Files Modified

- **pom.xml**: Updated repository endpoints from new Central API to OSSRH legacy
- **~/.m2/settings.xml**: Already configured with OSSRH credentials

---

## Next Steps

1. **Check Sonatype Account Status**
   - URL: https://identity.sonatype.com/
   - Verify account is active and has upload permissions

2. **Verify Namespace Registration**
   - GroupId `io.github.arunkatherashala` must be registered
   - If not, will need JIRA approval process

3. **If Successful**
   - Re-run: `mvn clean deploy -DskipTests`
   - Monitor staging repository: https://oss.sonatype.org
   - Release when ready

4. **If Blocked**
   - Use alternative groupId
   - Or contact Sonatype support
   - Package can still be deployed to Crates.io, npm, NuGet, RubyGems

---

## Maven Commands for Reference

```powershell
# Set Maven location
$maven = "C:\Maven\apache-maven-3.9.6\bin\mvn.cmd"

# Clean and build
& $maven clean install -DskipTests

# Deploy to Maven Central
& $maven clean deploy -DskipTests

# Deploy with verbose logging
& $maven clean deploy -DskipTests -X

# Test repository connectivity
& $maven test -X | Select-String "ossrh|401|402|403"
```

---

## Sonatype OSSRH vs Central

| Aspect | OSSRH (Legacy) | Central (New) |
|--------|---|---|
| URL | oss.sonatype.org | central.sonatype.com |
| Protocol | Maven traditional | REST API |
| Ease | Well-documented | Newer, may have issues |
| Support | Mature | Growing |
| Recommendation | Use for Maven | Use for web uploads |

---

**Status:** Maven infrastructure is ready. Awaiting Sonatype account verification and namespace approval.

