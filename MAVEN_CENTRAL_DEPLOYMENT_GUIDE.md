# Maven Central Deployment Guide via Sonatype

## Prerequisites
- Maven installed (3.6.0+)
- Sonatype JIRA account (username: je76g3)
- Sonatype credentials configured

## Step 1: Configure Maven Settings

Your `~/.m2/settings.xml` is already configured with:
```xml
<server>
  <id>ossrh</id>
  <username>je76g3</username>
  <password>iAyH2RS1ZK3PFBbOksv6oRRe37jwVRQNH</password>
</server>
```

## Step 2: Install Maven (Windows PowerShell)

### Option A: Using Chocolatey (Recommended)
```powershell
# If you have Chocolatey installed:
choco install maven
```

### Option B: Manual Download
```powershell
# 1. Download Maven from https://maven.apache.org/download.cgi
# 2. Extract to a directory (e.g., C:\Maven)
# 3. Add to PATH:
[Environment]::SetEnvironmentVariable("MAVEN_HOME", "C:\Maven\apache-maven-3.9.6", [EnvironmentVariableTarget]::User)
[Environment]::SetEnvironmentVariable("Path", $env:Path + ";C:\Maven\apache-maven-3.9.6\bin", [EnvironmentVariableTarget]::User)
# 4. Verify:
mvn --version
```

### Option C: Using Windows Package Manager
```powershell
winget install Maven.Maven
```

### Option D: Using Windows Subsystem for Linux (WSL)
```bash
sudo apt update
sudo apt install maven
mvn --version
```

## Step 3: Verify pom.xml Configuration

File: `C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\pom.xml`

Required elements:
- ✅ Version: 1.3.3
- ✅ GroupId: io.github.arunkatherashala
- ✅ ArtifactId: kore-fileformat
- ✅ Distribution Management (ossrh endpoints)
- ✅ SCM configuration
- ✅ Developer information

## Step 4: Deploy to Maven Central

```powershell
cd "C:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore"

# Build and deploy
mvn clean deploy -DskipTests

# Or for more verbose output:
mvn clean deploy -DskipTests -X
```

## Expected Output

```
[INFO] Uploading to ossrh: https://central.sonatype.com/api/v1/publisher/upload
[INFO] Downloading from ossrh: https://central.sonatype.com/api/v1/publisher/upload
[INFO] Uploaded to ossrh: https://central.sonatype.com/api/v1/publisher/upload
[INFO] BUILD SUCCESS
```

## Step 5: Monitor Release

1. Go to: https://central.sonatype.com/
2. Click "Staging Repositories"
3. Find your repository: io.github.arunkatherashala-XXXX
4. Review contents
5. Click "Release" button
6. Wait 10-30 minutes for syncing

## Troubleshooting

### 403 Forbidden Error
- Credentials incorrect in settings.xml
- Account doesn't have upload permissions
- Solution: Verify at https://identity.sonatype.com/

### Build Fails with Missing Dependencies
```powershell
mvn clean install
```

### GPG Signing Issues
If you have GPG signing configured, you may need:
```powershell
mvn clean deploy -DskipTests -Dgpg.skip=true
```

## Verification

Once deployed and synced:
```bash
# Search Maven Central
https://central.sonatype.com/search?q=kore-fileformat

# Or use Maven:
mvn dependency:get -Dartifact=io.github.arunkatherashala:kore-fileformat:1.3.3:jar
```

## Additional Resources

- Sonatype OSSRH Guide: https://central.sonatype.org/publishing/publish-maven/
- Sonatype Identity: https://identity.sonatype.com/
- Maven Central Search: https://central.sonatype.com/
- Maven Documentation: https://maven.apache.org/

