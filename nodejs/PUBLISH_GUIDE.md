# Publishing KORE to npm

This guide walks you through publishing the KORE Node.js bindings to npm.

## Step 1: Prepare Your npm Account

If you don't have an npm account, create one at https://www.npmjs.com/signup

Once created, authenticate locally:

```bash
npm login
```

This will prompt you for:
- Username
- Password
- Email

Your credentials are saved in `~/.npmrc`.

## Step 2: Update Version

Update the version in `package.json`:

```json
{
  "name": "kore-fileformat",
  "version": "0.4.0",  // ← Update this
  ...
}
```

Follow semantic versioning:
- **Patch**: 0.4.1 (bug fixes)
- **Minor**: 0.5.0 (new features, backward compatible)
- **Major**: 1.0.0 (breaking changes)

## Step 3: Build the Bindings

```bash
cd nodejs
npm install
npm run build
```

This compiles the Rust code to native bindings for your platform.

## Step 4: Test Locally

```bash
npm test
```

All tests must pass before publishing.

## Step 5: Publish to npm

### Option A: Manual Publish

```bash
npm publish
```

### Option B: Using Script (Recommended)

**On Linux/macOS:**
```bash
chmod +x publish.sh
./publish.sh
```

**On Windows:**
```cmd
publish.bat
```

### Option C: Using GitHub Actions

Push a tag to trigger automatic publishing:

```bash
git tag v0.4.0
git push origin v0.4.0
```

This triggers the `.github/workflows/publish-nodejs.yml` workflow which:
1. Builds bindings
2. Runs tests
3. Publishes to npm
4. Creates a GitHub release

## Step 6: Verify Publication

After publishing, verify the package is available:

```bash
npm view kore-fileformat
```

Or visit: https://www.npmjs.com/package/kore-fileformat

## Step 7: Users Can Install

Users can now install with:

```bash
npm install kore-fileformat
```

## Pre-release Publishing

To publish a pre-release version (e.g., 0.4.0-beta.1):

```bash
# Update package.json version to "0.4.0-beta.1"
npm publish --tag beta
```

Users can then install with:

```bash
npm install kore-fileformat@beta
```

## Publishing for Multiple Platforms

npm automatically detects and publishes binaries for:

- Linux (x86_64, ARM64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64)

To build for all platforms, use GitHub Actions by pushing tags, or build manually on each platform and publish together using the napi-cli tooling.

## Troubleshooting

### Error: "You must be logged in to publish packages"

```bash
npm login
```

### Error: "Package name already taken"

Choose a unique package name in `package.json`:

```json
{
  "name": "@yourusername/kore-fileformat",
  ...
}
```

Then publish to npm org:

```bash
npm publish --access public
```

### Error: "Permission denied"

Ensure you have permission to publish the package. You may need to:
1. Be the package owner
2. Be added as a collaborator
3. Join the npm organization

### Error: "Version X already published"

Update to a new version in `package.json`:

```bash
npm version patch  # 0.4.0 → 0.4.1
npm version minor  # 0.4.0 → 0.5.0
npm version major  # 0.4.0 → 1.0.0
```

Then publish:

```bash
npm publish
```

## Publishing Checklist

- [ ] Updated version in `package.json`
- [ ] Built successfully: `npm run build`
- [ ] Tests pass: `npm test`
- [ ] Authenticated: `npm login`
- [ ] No pending git changes
- [ ] Pushed to GitHub (if using GitHub Actions)
- [ ] Published: `npm publish`
- [ ] Verified on npm: `npm view kore-fileformat`

## What Gets Published

The npm package includes:

✅ JavaScript wrapper (`index.js`)  
✅ TypeScript types (`index.d.ts`)  
✅ README and documentation  
✅ Prebuilt native bindings  
✅ Examples  

❌ Rust source code (excluded via `.npmignore`)  
❌ Tests (excluded via `.npmignore`)  
❌ Build artifacts  

## After Publishing

1. **Announce It**
   - Share on Twitter/LinkedIn
   - Post on Dev.to, Medium, Hashnode
   - Update GitHub releases

2. **Gather Feedback**
   - Monitor npm download stats
   - Check GitHub issues
   - Read user discussions

3. **Plan Next Release**
   - Collect feature requests
   - Fix bugs
   - Performance improvements

---

**Ready to publish?** Follow the steps above! 🚀
