#!/bin/bash

# KORE Node.js Binding Build Script
# Builds and publishes the npm package

set -e

echo "🔨 Building KORE Node.js bindings..."

# Install dependencies
npm install

# Build release binaries
npm run build

# Run tests
echo "🧪 Running tests..."
npm test

# Check if .npmrc exists for authentication
if [ ! -f "$HOME/.npmrc" ]; then
    echo "⚠️  No .npmrc found. Please run 'npm login' first."
    exit 1
fi

# Publish to npm
echo "📦 Publishing to npm..."
npm publish

echo "✅ Successfully published kore-fileformat@$(jq -r .version package.json)"
echo "🎉 Check it out: https://www.npmjs.com/package/kore-fileformat"
