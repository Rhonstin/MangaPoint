# Security

## Reporting a vulnerability

Please **do not open a public issue** for a security problem. Use
[**Report a vulnerability**](https://github.com/W1seGit/MangaPoint/security/advisories/new)
on the repository's Security tab, so it can be discussed privately and fixed before
it is announced. Say what you found, how to reproduce it, and which version you
used (Settings → About).

## What MangaPoint does on your computer

- It starts the Suwayomi catalog server on `127.0.0.1` only, protected by a
  random password made at every launch, and stops it when the app closes.
- It downloads that server (with its own Java) from the official Suwayomi release
  and checks it against a pinned SHA-256 before using it.
- It talks to the sources you install and, if you choose Wi-Fi export, to the
  reader address you type. It has no account, no analytics and no server of ours.
- Everything it stores is in the folder described in the README.

Only the latest release is supported with fixes.
