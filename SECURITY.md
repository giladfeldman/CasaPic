# Security

CasaPic is a desktop program that works offline, except for its update check. It opens image
files from folders you choose, so a specially crafted image file is the main way it could be
attacked. The update check reads a release file from CasaPic's GitHub page and installs an update
only if its signature matches CasaPic's own key; it sends nothing about your photos or library,
and it can be turned off in Tools ▸ Options ▸ Updates.

## Reporting a problem

Please do not open a public issue for a security problem. Use GitHub's private reporting
instead: on the repository page, go to **Security ▸ Report a vulnerability**.

Include the CasaPic version (Help ▸ About), what the problem is, and how to reproduce it. If
a particular file triggers it, say how to make one; do not send personal photos.

## Supported versions

Only the newest release receives fixes.
