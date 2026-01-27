# Releasing probe-verus

This document describes how to create releases for probe-verus. A single release publishes both the GitHub Action and the Docker image.

## Versioning Strategy

We use [semantic versioning](https://semver.org/) with floating major version tags for the GitHub Action:

- **Semver tags** (`v1.0.0`, `v1.1.0`, `v2.0.0`): Specific releases
- **Floating tags** (`v1`, `v2`): Always point to the latest release in that major version
- **Docker `latest` tag**: Points to the most recent release

Users reference the floating tag in their workflows (`@v1`) to automatically receive minor and patch updates.

## What Gets Published

When you create a release, the `docker-publish.yml` workflow automatically publishes Docker images with these tags:

| Release Tag | Docker Tags Created |
|-------------|---------------------|
| `v1.0.0` | `latest`, `v1.0.0`, `1.0.0`, `1.0`, `<sha>` |
| `v1.1.0` | `latest`, `v1.1.0`, `1.1.0`, `1.1`, `<sha>` |
| `v2.0.0` | `latest`, `v2.0.0`, `2.0.0`, `2.0`, `<sha>` |

## Creating a New Release

### 1. Ensure main is ready

Make sure all changes are merged to `main` and CI is passing.

```bash
git checkout main
git pull origin main
```

### 2. Create the release

Use GitHub CLI or the GitHub web UI:

```bash
# Patch release (bug fixes)
gh release create v1.0.1 --title "v1.0.1" --notes "Bug fixes and improvements"

# Minor release (new features, backwards compatible)
gh release create v1.1.0 --title "v1.1.0" --notes "New features..."

# Major release (breaking changes)
gh release create v2.0.0 --title "v2.0.0" --notes "Breaking changes..."
```

### 3. Update the floating major version tag

After creating the release, update the floating tag to point to it:

```bash
# Fetch the new tag
git fetch --tags

# Update floating tag (replace v1.0.1 with your new version)
git tag -f v1 v1.0.1
git push -f origin v1
```

For a new major version:

```bash
git tag v2 v2.0.0
git push origin v2
```

### 4. Verify the release

1. Check the [Actions tab](https://github.com/Beneficial-AI-Foundation/probe-verus/actions) to ensure the Docker publish workflow succeeded
2. Verify the [package page](https://github.com/Beneficial-AI-Foundation/probe-verus/pkgs/container/probe-verus) shows the new tags
3. Test the Docker image:
   ```bash
   docker pull ghcr.io/beneficial-ai-foundation/probe-verus:latest
   docker pull ghcr.io/beneficial-ai-foundation/probe-verus:v1.0.1
   ```

## Release Checklist

- [ ] All changes merged to `main`
- [ ] CI passing on `main`
- [ ] Created GitHub release with semver tag (e.g., `v1.0.1`)
- [ ] Updated floating major version tag (e.g., `v1`)
- [ ] Docker publish workflow succeeded
- [ ] Docker images available on GHCR with correct tags
- [ ] `latest` tag points to new release

## Troubleshooting

### Docker image missing `latest` tag

The `latest` tag is only applied on release events. If you pushed a tag manually without creating a GitHub release, the workflow won't apply `latest`. Create a proper GitHub release instead.

### Floating tag out of sync

If the floating tag (`v1`) doesn't point to the latest release:

```bash
git fetch --tags
git tag -f v1 v1.x.x  # Replace with latest version
git push -f origin v1
```

### Need to delete a release

```bash
# Delete GitHub release
gh release delete v1.0.1

# Delete the tag
git push origin --delete v1.0.1
git tag -d v1.0.1
```
