# Security policy

Please do not open a public issue for a security problem. Report it through
[GitHub private vulnerability reporting](https://github.com/chansuke/silon/security/advisories/new).

Only the latest commit on `main` gets fixes.

The introspection API (the `introspect` feature) lets any local process read
the app state and dispatch actions. It binds to 127.0.0.1 by default and is
meant for development builds, so do not enable it in a release build.
