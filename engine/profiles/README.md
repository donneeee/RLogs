# Character profiles

Game-neutral, bounded local packages for privacy-reviewed character snapshots.

A trusted game plug-in projects reviewed canonical events into a
`WebsitePayloadRequest`. Core then wraps that request with sealed-log source
evidence and a deterministic payload digest. The request digest is computed
from compact JSON with every object key recursively sorted, so the website and
future backends can reproduce the same seal without relying on Rust struct
field order. The package has no website host, authentication, account
container, credential, login token, or transport state.

Packages are review artifacts, not permission to transmit. The desktop stores
them only when the corresponding game profile-sync policy is enabled. A future
transport must check consent again immediately before sending.
